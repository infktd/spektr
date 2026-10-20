//! The TUI's `Host`: recent cases, the doctor, opening and creating cases.
//! The TUI never sees paths, config or the store directly.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use futures::future::BoxFuture;
use spektr_engine::{EngineHandle, Profiles};
use spektr_model::EntityFlags;
use spektr_store::Db;
use spektr_tui::{Chain, DoctorLine, External, Host, Level, NewCase, RecentCase, Settings};

use crate::cli::NewArgs;
use crate::commands::doctor;
use crate::wiring::{Ctx, Modules};

pub struct TuiHost {
    pub ctx: Arc<Ctx>,
}

impl TuiHost {
    fn chain(&self, path: &Path) -> Chain {
        if !path.exists() {
            return Chain::Missing("missing".into());
        }
        let opts = match self.ctx.store_options() {
            Ok(o) => o,
            Err(e) => return Chain::Missing(e.to_string()),
        };
        match Db::open_read_only(path, opts).and_then(|mut db| db.verify()) {
            Ok(r) => match r.failure {
                None => Chain::Ok { rows: r.audit_rows },
                Some(f) => Chain::Broken {
                    at: match f.location {
                        spektr_store::VerifyLocation::AuditRow(seq) => format!("#{seq}"),
                        other => other.to_string(),
                    },
                },
            },
            Err(e) => Chain::Missing(e.to_string()),
        }
    }

    fn info(&self, path: &Path, opened: jiff::Timestamp) -> RecentCase {
        let mut rc = RecentCase {
            path: path.to_path_buf(),
            name: path
                .file_stem()
                .map_or_else(String::new, |s| s.to_string_lossy().into_owned()),
            entities: None,
            edges: None,
            last_opened: Some(opened),
            chain: Chain::Unknown,
            size: std::fs::metadata(path).ok().map(|m| m.len()),
            seeds: Vec::new(),
        };
        let Ok(opts) = self.ctx.store_options() else {
            return rc;
        };
        if let Ok(mut db) = Db::open_read_only(path, opts) {
            let _ = db.read(|r| {
                rc.entities = r.entity_count().ok();
                rc.edges = r.edge_count().ok();
                if let Ok(Some(serde_json::Value::String(n))) = r.meta("name") {
                    rc.name = n;
                }
                rc.seeds = r
                    .all_entities()?
                    .into_iter()
                    .filter(|e| e.merged_into.is_none() && e.flags.contains(EntityFlags::SEED))
                    .map(|e| (e.ty, e.value))
                    .collect();
                Ok(())
            });
        }
        rc.chain = self.chain(path);
        rc
    }
}

impl Host for TuiHost {
    fn recent(&self) -> Vec<RecentCase> {
        crate::recent::load(&self.ctx.paths)
            .into_iter()
            .map(|e| self.info(&e.path, e.opened))
            .collect()
    }

    fn doctor(&self) -> Vec<DoctorLine> {
        doctor::checks(&self.ctx)
            .into_iter()
            .filter(|c| !matches!(c.name.as_str(), "terminal" | "images" | "state"))
            .map(|c| DoctorLine {
                name: c.name,
                level: match c.level {
                    doctor::Level::Ok => Level::Ok,
                    doctor::Level::Warn => Level::Warn,
                    doctor::Level::Fail => Level::Fail,
                },
                detail: c.detail,
            })
            .collect()
    }

    fn verify(&self, path: &Path) -> Chain {
        self.chain(path)
    }

    fn profiles(&self) -> Vec<String> {
        let Ok(mut p) = Profiles::builtin() else {
            return Vec::new();
        };
        let _ = p.load_dir(&self.ctx.paths.config.join("profiles"));
        p.names().map(str::to_string).collect()
    }

    fn open(&self, path: PathBuf) -> BoxFuture<'_, Result<EngineHandle, String>> {
        Box::pin(async move {
            if !path.exists() {
                return Err(format!("{} not found", path.display()));
            }
            self.ctx
                .open(&path, None, Modules::All)
                .await
                .map_err(|e| format!("{e:#}"))
        })
    }

    fn create(&self, req: NewCase) -> BoxFuture<'_, Result<(PathBuf, EngineHandle), String>> {
        Box::pin(async move {
            let args = NewArgs {
                name: req.name,
                profile: req.profile,
                subject: req.subject,
                last_seen: req.last_seen,
                region: "US".into(),
                depth: None,
                scope: Vec::new(),
                dir: None,
            };
            crate::commands::new::create(&self.ctx, &args, Modules::All)
                .await
                .map(|(p, _, h)| (p, h))
                .map_err(|e| format!("{e:#}"))
        })
    }

    fn opened(&self, path: &Path) {
        crate::recent::touch(&self.ctx.paths, path, self.ctx.now());
    }

    fn now(&self) -> jiff::Timestamp {
        self.ctx.now()
    }

    fn external(&self, what: &External) -> Result<Option<String>, String> {
        let (kind, target) = match what {
            External::Url(u) => ("url", u.clone()),
            External::Open(p) => ("open", p.display().to_string()),
            External::Reveal(p) => ("reveal", p.display().to_string()),
            External::Pager(p) => ("pager", p.display().to_string()),
        };
        // Test mode (ARCHITECTURE §8): nothing reaches the OS; the request
        // is recorded for the E2E tests.
        if self.ctx.test.test_mode {
            record_open(&self.ctx.paths.state, kind, &target).map_err(|e| e.to_string())?;
            return Ok(Some(target));
        }
        let r = match what {
            External::Url(u) => open::that_detached(u),
            External::Open(p) => open::that_detached(p),
            External::Reveal(p) => reveal(p),
            External::Pager(p) => pager(p),
        };
        r.map(|()| None)
            .map_err(|e| format!("opening {target}: {e}"))
    }

    fn pager_takes_terminal(&self) -> bool {
        !self.ctx.test.test_mode
    }
}

/// Append `{"kind", "target"}` to `<state>/opened.jsonl`.
fn record_open(state: &Path, kind: &str, target: &str) -> std::io::Result<()> {
    use std::io::Write;
    std::fs::create_dir_all(state)?;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(state.join("opened.jsonl"))?;
    writeln!(
        f,
        "{}",
        serde_json::json!({ "kind": kind, "target": target })
    )
}

/// Select a file in the system file manager.
fn reveal(p: &Path) -> std::io::Result<()> {
    if cfg!(target_os = "macos") {
        std::process::Command::new("open")
            .arg("-R")
            .arg(p)
            .spawn()
            .map(|_| ())
    } else if cfg!(windows) {
        std::process::Command::new("explorer")
            .arg(format!("/select,{}", p.display()))
            .spawn()
            .map(|_| ())
    } else {
        open::that_detached(p.parent().unwrap_or(p))
    }
}

/// `$PAGER` (default `less -R`), waiting for it to exit.
fn pager(p: &Path) -> std::io::Result<()> {
    let cmd = std::env::var("PAGER")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| "less -R".into());
    let mut parts = cmd.split_whitespace();
    let prog = parts.next().unwrap_or("less");
    std::process::Command::new(prog)
        .args(parts)
        .arg(p)
        .status()
        .map(|_| ())
}

/// TUI settings from config.toml and the environment.
pub fn settings(ctx: &Ctx) -> Settings {
    let c = &ctx.config;
    let keys = c
        .keys
        .iter()
        .map(|(k, v)| {
            let list = match v {
                toml::Value::String(s) => vec![s.clone()],
                toml::Value::Array(a) => a
                    .iter()
                    .filter_map(|x| x.as_str().map(str::to_string))
                    .collect(),
                _ => Vec::new(),
            };
            (k.clone(), list)
        })
        .collect();
    // Test mode defaults to halfblocks, so frames don't depend on the
    // terminal; config can still ask for a protocol (the real-terminal QA).
    let images = c
        .images
        .as_deref()
        .and_then(spektr_tui::ImageMode::parse)
        .unwrap_or(if ctx.test.test_mode {
            spektr_tui::ImageMode::Halfblocks
        } else {
            spektr_tui::ImageMode::Auto
        });
    Settings {
        script: ctx.test.keys.clone().filter(|_| ctx.test.test_mode),
        mouse: c.mouse.unwrap_or(true),
        images,
        glyphs: c
            .glyphs
            .as_deref()
            .and_then(spektr_tui::glyph::GlyphSet::parse)
            .unwrap_or_default(),
        colors: spektr_tui::theme::ColorMode::detect(
            std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty()),
            c.colors.as_deref(),
            std::env::var("COLORTERM").ok().as_deref(),
        ),
        keys,
        version: env!("CARGO_PKG_VERSION").to_string(),
        home: dirs::home_dir(),
    }
}
