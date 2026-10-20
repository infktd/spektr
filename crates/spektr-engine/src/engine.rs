//! Engine construction: open a case, register modules (built-ins are
//! injected by the binary; exec modules are loaded from the config dir),
//! then `start` the scheduler and ingestion tasks.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use spektr_model::{Clock, EntityId, ExtNormalize, Vocabulary};
use spektr_protocol::Manifest;
use spektr_store::{CaseInit, OpenOptions, Store};
use tokio::sync::{broadcast, mpsc};

use crate::capture::CaptureService;
use crate::correlate::{
    AnalyticsIdCorrelator, AvatarPhashCorrelator, CertSanCorrelator, Correlator, FaviconCorrelator,
    IpCorrelator, RegistrantEmailCorrelator, SamePersonCandidates,
};
use crate::error::{EngineError, EngineResult};
use crate::events::{EngineEvent, EventKind};
use crate::exec::ExecModule;
use crate::handle::EngineHandle;
use crate::ingest::{IngestMsg, Ingestor};
use crate::module::{JobLimits, Module};
use crate::profile::{Profiles, RulesProfile};
use crate::quiesce::Work;
use crate::scheduler::{SchedMsg, Scheduler};

/// Engine configuration, resolved by the binary.
#[derive(Debug, Clone)]
pub struct EngineConfig {
    pub store: OpenOptions,
    /// `<config>`: holds `modules/` and `profiles/`.
    pub config_dir: Option<PathBuf>,
    /// From `config.toml [secrets]` and `SPEKTR_SECRET_<NAME>` (env wins).
    pub secrets: BTreeMap<String, String>,
    /// `[modules.<name>]` tables from config.toml.
    pub module_config: BTreeMap<String, serde_json::Value>,
    pub limits: JobLimits,
    /// Global job semaphore (ARCHITECTURE §3: 16).
    pub concurrency: usize,
    /// `spektr run --depth`.
    pub depth_override: Option<u32>,
    pub batch_max: usize,
    pub batch_window: Duration,
    /// Exec module processes inherit this environment (tests set PATH).
    pub exec_env: BTreeMap<String, String>,
}

impl EngineConfig {
    pub fn new(store: OpenOptions) -> Self {
        Self {
            store,
            config_dir: None,
            secrets: BTreeMap::new(),
            module_config: BTreeMap::new(),
            limits: JobLimits::default(),
            concurrency: 16,
            depth_override: None,
            batch_max: 200,
            batch_window: Duration::from_millis(50),
            exec_env: BTreeMap::new(),
        }
    }
}

/// Case settings from `case_meta`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaseSettings {
    pub name: String,
    pub profile: String,
    pub depth: u32,
    pub region: String,
    pub hub_threshold: u32,
    pub last_seen: Option<String>,
    pub subject: Option<EntityId>,
}

pub(crate) struct Shared {
    pub store: Store,
    pub clock: Clock,
    pub config: EngineConfig,
    pub vocab: RwLock<Arc<Vocabulary>>,
    pub modules: RwLock<BTreeMap<String, Arc<dyn Module>>>,
    pub manifest_hashes: RwLock<BTreeMap<String, String>>,
    pub exec_names: RwLock<BTreeSet<String>>,
    pub profiles: RwLock<Profiles>,
    pub capture: RwLock<Option<Arc<dyn CaptureService>>>,
    pub correlators: Vec<Box<dyn Correlator>>,
    pub events: broadcast::Sender<EngineEvent>,
    pub sched: mpsc::UnboundedSender<SchedMsg>,
    pub ingest: mpsc::Sender<IngestMsg>,
    pub work: Work,
    pub case: RwLock<CaseSettings>,
    pub warnings: Mutex<Vec<String>>,
    pub hub_notified: Mutex<BTreeSet<EntityId>>,
}

// Locks here are never held across an await, and a poisoned lock only means
// another thread panicked while holding it; the data is still consistent
// for these read-mostly maps, so recover the guard instead of panicking.
macro_rules! read {
    ($l:expr) => {
        $l.read().unwrap_or_else(|e| e.into_inner())
    };
}
macro_rules! write {
    ($l:expr) => {
        $l.write().unwrap_or_else(|e| e.into_inner())
    };
}
pub(crate) use read;

impl Shared {
    pub fn emit(&self, kind: EventKind) {
        let _ = self.events.send(EngineEvent {
            at: self.clock.now(),
            kind,
        });
    }

    pub fn module(&self, name: &str) -> Option<Arc<dyn Module>> {
        read!(self.modules).get(name).cloned()
    }

    pub fn vocab(&self) -> Arc<Vocabulary> {
        Arc::clone(&read!(self.vocab))
    }

    pub fn settings(&self) -> CaseSettings {
        read!(self.case).clone()
    }

    /// The case's active rules profile.
    pub fn profile(&self) -> EngineResult<RulesProfile> {
        let name = self.settings().profile;
        read!(self.profiles)
            .get(&name)
            .cloned()
            .ok_or_else(|| EngineError::Profile(format!("unknown rules profile `{name}`")))
    }

    pub fn depth_limit(&self) -> u32 {
        self.config.depth_override.unwrap_or(self.settings().depth)
    }

    pub fn manifest_hash(&self, module: &str) -> String {
        read!(self.manifest_hashes)
            .get(module)
            .cloned()
            .unwrap_or_default()
    }

    /// Required secrets the module declares that aren't set.
    pub fn missing_secrets(&self, m: &Manifest) -> Vec<String> {
        m.secrets
            .iter()
            .filter(|s| !self.config.secrets.contains_key(*s))
            .cloned()
            .collect()
    }

    pub fn warn(&self, w: String) {
        tracing::warn!("{w}");
        self.warnings
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(w);
    }

    /// Re-read case settings from `case_meta`.
    pub async fn reload_settings(&self) -> EngineResult<()> {
        let s = load_settings(&self.store).await?;
        *write!(self.case) = s;
        Ok(())
    }
}

pub(crate) async fn load_settings(store: &Store) -> EngineResult<CaseSettings> {
    Ok(store
        .read(|r| {
            let m = r.meta_all()?;
            let s = |k: &str| m.get(k).and_then(|v| v.as_str()).map(str::to_string);
            let n = |k: &str, d: u64| m.get(k).and_then(serde_json::Value::as_u64).unwrap_or(d);
            Ok(CaseSettings {
                name: s("name").unwrap_or_default(),
                profile: s("rules_profile").unwrap_or_else(|| "default".into()),
                depth: u32::try_from(n("default_depth", 2)).unwrap_or(2),
                region: s("default_region").unwrap_or_else(|| "US".into()),
                hub_threshold: u32::try_from(n("hub_threshold", 50)).unwrap_or(50),
                last_seen: s("last_seen"),
                subject: s("subject_entity").and_then(|v| v.parse().ok()),
            })
        })
        .await?)
}

/// A configured engine, not yet running.
pub struct Engine {
    shared: Arc<Shared>,
    sched_rx: mpsc::UnboundedReceiver<SchedMsg>,
    ingest_rx: mpsc::Receiver<IngestMsg>,
}

impl Engine {
    /// Open an existing case.
    pub async fn open(path: &Path, config: EngineConfig) -> EngineResult<Self> {
        let store = Store::open(path, config.store.clone())?;
        Self::with_store(store, config).await
    }

    /// Create a new case (`spektr new`).
    pub async fn create(path: &Path, init: &CaseInit, config: EngineConfig) -> EngineResult<Self> {
        let store = Store::create(path, init, config.store.clone())?;
        Self::with_store(store, config).await
    }

    async fn with_store(store: Store, config: EngineConfig) -> EngineResult<Self> {
        let case = load_settings(&store).await?;
        let mut profiles = Profiles::builtin()?;
        let mut warnings = Vec::new();
        if let Some(dir) = &config.config_dir {
            warnings.extend(profiles.load_dir(&dir.join("profiles"))?);
        }
        let (events, _) = broadcast::channel(4096);
        let (sched, sched_rx) = mpsc::unbounded_channel();
        let (ingest, ingest_rx) = mpsc::channel(1024);
        let shared = Arc::new(Shared {
            clock: config.store.clock,
            store,
            config,
            vocab: RwLock::new(Arc::new(Vocabulary::builtin())),
            modules: RwLock::new(BTreeMap::new()),
            manifest_hashes: RwLock::new(BTreeMap::new()),
            exec_names: RwLock::new(BTreeSet::new()),
            profiles: RwLock::new(profiles),
            capture: RwLock::new(None),
            correlators: vec![
                Box::new(AvatarPhashCorrelator),
                Box::new(AnalyticsIdCorrelator),
                Box::new(FaviconCorrelator),
                Box::new(RegistrantEmailCorrelator),
                Box::new(CertSanCorrelator),
                Box::new(IpCorrelator),
                Box::new(SamePersonCandidates),
            ],
            events,
            sched,
            ingest,
            work: Work::default(),
            case: RwLock::new(case),
            warnings: Mutex::new(Vec::new()),
            hub_notified: Mutex::new(BTreeSet::new()),
        });
        for w in warnings {
            shared.warn(w);
        }
        Ok(Self {
            shared,
            sched_rx,
            ingest_rx,
        })
    }

    /// Register a module (built-ins from the binary, or exec modules).
    /// Declares its platforms, extension types and predicates.
    pub fn register(&self, m: Box<dyn Module>) -> EngineResult<()> {
        register(&self.shared, Arc::from(m))
    }

    pub fn set_capture(&self, c: impl CaptureService) {
        *write!(self.shared.capture) = Some(Arc::new(c));
    }

    /// Load every `<dir>/<name>/module.toml`. Problems are warnings (shown by
    /// the doctor), not errors.
    pub fn load_exec_modules(&self, dir: &Path) -> Vec<String> {
        load_exec_modules(&self.shared, dir)
    }

    /// Warnings collected while loading profiles and modules.
    pub fn warnings(&self) -> Vec<String> {
        self.shared
            .warnings
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Spawn the scheduler and ingestion tasks. Must be called inside a
    /// tokio runtime.
    pub fn start(self) -> EngineHandle {
        let Engine {
            shared,
            sched_rx,
            ingest_rx,
        } = self;
        tokio::spawn(Scheduler::new(Arc::clone(&shared), sched_rx).run());
        tokio::spawn(Ingestor::new(Arc::clone(&shared), ingest_rx).run());
        EngineHandle::new(shared)
    }
}

pub(crate) fn register(shared: &Arc<Shared>, m: Arc<dyn Module>) -> EngineResult<()> {
    let manifest = m.manifest().clone();
    manifest
        .check()
        .map_err(|e| EngineError::Invalid(format!("module {}: {e}", manifest.name)))?;
    if read!(shared.modules).contains_key(&manifest.name) {
        return Err(EngineError::Invalid(format!(
            "module `{}` is already registered",
            manifest.name
        )));
    }
    {
        let mut vocab = (*shared.vocab()).clone();
        for p in &manifest.platforms {
            vocab
                .platforms
                .declare(&p.id, p.case_sensitive, p.profile_url.as_deref())
                .map_err(|e| {
                    EngineError::Invalid(format!(
                        "module {}: platform {}: {e}",
                        manifest.name, p.id
                    ))
                })?;
        }
        for t in &manifest.types {
            let rule = match t.normalize.as_str() {
                "exact" => ExtNormalize::Exact,
                "trim" => ExtNormalize::Trim,
                "lowercase" => ExtNormalize::Lowercase,
                "casefold" => ExtNormalize::Casefold,
                _ => ExtNormalize::Url,
            };
            vocab.declare_type(&manifest.name, &t.name, rule)?;
        }
        for p in &manifest.predicates {
            let src: Vec<&str> = p.src.iter().map(String::as_str).collect();
            let dst: Vec<&str> = p.dst.iter().map(String::as_str).collect();
            vocab.declare_predicate(&manifest.name, &p.name, &src, &dst)?;
        }
        // Every produced pair must name known vocabulary.
        for (pred, ty) in &manifest.produces {
            vocab.predicate(pred)?;
            vocab.entity_type(ty)?;
        }
        for c in &manifest.consumes {
            let ty = c.split_once(':').map_or(c.as_str(), |(t, _)| t);
            vocab.entity_type(ty)?;
        }
        *write!(shared.vocab) = Arc::new(vocab);
    }
    let hash = serde_json::to_string(&manifest)
        .map(|s| spektr_store::sha256(s.as_bytes()).hex()[..16].to_string())
        .unwrap_or_default();
    write!(shared.manifest_hashes).insert(manifest.name.clone(), hash);
    write!(shared.modules).insert(manifest.name.clone(), m);
    Ok(())
}

pub(crate) fn load_exec_modules(shared: &Arc<Shared>, dir: &Path) -> Vec<String> {
    let mut warnings = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return warnings;
    };
    let mut dirs: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    for d in dirs {
        let toml_path = d.join("module.toml");
        let src = match std::fs::read_to_string(&toml_path) {
            Ok(s) => s,
            Err(e) => {
                warnings.push(format!("{}: {e}", toml_path.display()));
                continue;
            }
        };
        let manifest = match Manifest::from_toml(&src) {
            Ok(m) => m,
            Err(e) => {
                warnings.push(format!("{}: {e}", toml_path.display()));
                continue;
            }
        };
        if manifest.runtime != spektr_protocol::Runtime::Exec {
            warnings.push(format!(
                "{}: only exec modules can be installed",
                toml_path.display()
            ));
            continue;
        }
        let name = manifest.name.clone();
        let module = ExecModule::new(
            manifest,
            d.clone(),
            shared.config.exec_env.clone(),
            shared.config.secrets.values().cloned().collect(),
        );
        match register(shared, Arc::new(module)) {
            Ok(()) => {
                write!(shared.exec_names).insert(name);
            }
            Err(e) => warnings.push(format!("{}: {e}", toml_path.display())),
        }
    }
    for w in &warnings {
        shared.warn(w.clone());
    }
    warnings
}
