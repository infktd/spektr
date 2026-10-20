//! The TUI's `Host`: recent cases, the doctor, opening and creating cases.
//! The TUI never sees paths, config or the store directly.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use futures::future::BoxFuture;
use spektr_engine::{EngineHandle, Profiles};
use spektr_model::EntityFlags;
use spektr_store::Db;
use spektr_tui::{Chain, DoctorLine, Host, Level, NewCase, RecentCase, Settings};

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
    Settings {
        mouse: c.mouse.unwrap_or(true),
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
