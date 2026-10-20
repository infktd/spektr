//! Engine test helpers: a case in a temp dir and a scripted native module.

#![allow(dead_code, clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use futures::future::BoxFuture;
use serde_json::json;
use spektr_engine::{
    CaseInit, Engine, EngineConfig, EngineEvent, EngineHandle, JobInput, Module, ModuleCx,
    ModuleError, OpenOptions,
};
use spektr_model::{Clock, IdGen};
use spektr_protocol::{ErrorCode, Manifest, ModuleMessage};
use tempfile::TempDir;
use tokio::sync::broadcast;

pub fn opts() -> OpenOptions {
    OpenOptions::new(
        Clock::Fixed("2026-10-20T18:14:22Z".parse().unwrap()),
        IdGen::seeded(1),
    )
    .unwrap()
}

pub struct TestCase {
    pub dir: TempDir,
    pub path: PathBuf,
    pub handle: EngineHandle,
    pub events: broadcast::Receiver<EngineEvent>,
}

pub fn config(dir: &TempDir) -> EngineConfig {
    let mut c = EngineConfig::new(opts());
    c.config_dir = Some(dir.path().join("config"));
    c.batch_window = Duration::from_millis(5);
    c
}

/// Create a case, let `setup` register modules, start the engine.
pub async fn case_with(
    init: CaseInit,
    tweak: impl FnOnce(&mut EngineConfig),
    setup: impl FnOnce(&Engine),
) -> TestCase {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("case.spektr");
    let mut c = config(&dir);
    tweak(&mut c);
    let engine = Engine::create(&path, &init, c).await.unwrap();
    setup(&engine);
    let handle = engine.start();
    let events = handle.subscribe();
    TestCase {
        dir,
        path,
        handle,
        events,
    }
}

pub async fn case(setup: impl FnOnce(&Engine)) -> TestCase {
    case_with(CaseInit::named("test"), |_| {}, setup).await
}

pub fn manifest(toml_body: &str) -> Manifest {
    Manifest::from_toml(&format!("[module]\nruntime = \"native\"\ndescription = \"test module\"\nversion = \"1.0.0\"\n{toml_body}")).unwrap()
}

type Script = dyn Fn(&JobInput, u32) -> Result<Vec<ModuleMessage>, ModuleError> + Send + Sync;

/// A native module that emits whatever `script` returns for an input, then
/// returns its result. `script` also sees how many times it has run.
pub struct Scripted {
    pub manifest: Manifest,
    pub script: Box<Script>,
    pub runs: Arc<AtomicU32>,
    pub delay: Duration,
}

impl Scripted {
    pub fn new(
        manifest: Manifest,
        script: impl Fn(&JobInput, u32) -> Result<Vec<ModuleMessage>, ModuleError>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        Self {
            manifest,
            script: Box::new(script),
            runs: Arc::new(AtomicU32::new(0)),
            delay: Duration::ZERO,
        }
    }
}

impl Module for Scripted {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn run<'a>(&'a self, input: JobInput, cx: ModuleCx) -> BoxFuture<'a, Result<(), ModuleError>> {
        Box::pin(async move {
            let n = self.runs.fetch_add(1, Ordering::SeqCst);
            if !self.delay.is_zero() {
                tokio::select! {
                    _ = tokio::time::sleep(self.delay) => {}
                    _ = cx.cancel.cancelled() => return Err(ModuleError::Cancelled),
                }
            }
            for m in (self.script)(&input, n)? {
                cx.emit.emit(m).await?;
            }
            Ok(())
        })
    }
}

pub fn obs(id: &str, url: &str) -> ModuleMessage {
    ModuleMessage::Observation {
        job: None,
        id: id.into(),
        source_url: Some(url.into()),
        content_type: Some("application/json".into()),
        raw: Some(json!({"fictional": true}).to_string()),
        raw_b64: None,
        summary: None,
    }
}

pub fn entity(r: &str, ty: &str, value: &str, ev: &[&str]) -> ModuleMessage {
    ModuleMessage::Entity {
        job: None,
        reference: r.into(),
        entity: spektr_protocol::EntitySpec {
            ty: ty.into(),
            value: value.into(),
            blob: None,
            dedup_id: None,
            key: None,
        },
        evidence: ev.iter().map(|s| s.to_string()).collect(),
    }
}

pub fn edge(src: &str, p: &str, dst: &str, ev: &[&str]) -> ModuleMessage {
    ModuleMessage::Edge {
        job: None,
        src: src.into(),
        predicate: p.into(),
        dst: dst.into(),
        attrs: Default::default(),
        evidence: ev.iter().map(|s| s.to_string()).collect(),
    }
}

pub fn attr(
    target: &str,
    name: &str,
    value: spektr_protocol::AttrValue,
    event: bool,
    ev: &[&str],
) -> ModuleMessage {
    ModuleMessage::Attr {
        job: None,
        target: target.into(),
        name: name.into(),
        value,
        event,
        evidence: ev.iter().map(|s| s.to_string()).collect(),
    }
}

pub fn err(code: ErrorCode, msg: &str) -> ModuleError {
    ModuleError::code(code, msg)
}

/// Drain events received so far.
pub fn drain(rx: &mut broadcast::Receiver<EngineEvent>) -> Vec<EngineEvent> {
    let mut out = Vec::new();
    while let Ok(e) = rx.try_recv() {
        out.push(e);
    }
    out
}

pub fn names(events: &[EngineEvent]) -> Vec<String> {
    events
        .iter()
        .map(|e| {
            serde_json::to_value(&e.kind).unwrap()["event"]
                .as_str()
                .unwrap()
                .to_string()
        })
        .collect()
}

pub fn secrets(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}
