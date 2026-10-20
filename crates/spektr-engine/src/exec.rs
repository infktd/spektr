//! The exec runtime (MODULES §3): user modules in any language, over
//! JSON lines on stdin/stdout. stderr goes to the debug log, prefixed with
//! the module name and with secret values redacted; it is never parsed.
//!
//! - **Per-job mode** (`serve = false`): spawn, write `hello` and `run`, read
//!   until `done` or `error`, close stdin; kill if it hasn't exited 2 s later.
//! - **Serve mode** (`serve = true`): one long-lived process; `hello` →
//!   `ready`; many `run`s routed by job id; `cancel` per job.

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use base64::Engine as _;
use futures::future::BoxFuture;
use spektr_model::JobId;
use spektr_protocol::{
    ErrorCode, HostMessage, InputEntity, MAX_LINE, Manifest, ModuleMessage, PROTOCOL_VERSION,
    parse_duration,
};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::{Mutex, mpsc};

use crate::module::{JobInput, Module, ModuleCx, ModuleError};

/// How long a process gets to exit after its job ends.
const EXIT_GRACE: Duration = Duration::from_secs(2);

pub struct ExecModule {
    manifest: Manifest,
    dir: PathBuf,
    env: BTreeMap<String, String>,
    redact: Vec<String>,
    serve: Mutex<Option<Arc<ServeProc>>>,
}

impl ExecModule {
    pub fn new(
        manifest: Manifest,
        dir: PathBuf,
        env: BTreeMap<String, String>,
        secret_values: Vec<String>,
    ) -> Self {
        Self {
            manifest,
            dir,
            env,
            redact: secret_values.into_iter().filter(|s| s.len() >= 4).collect(),
            serve: Mutex::new(None),
        }
    }

    fn command(&self) -> Result<Command, ModuleError> {
        let (prog, args) = self
            .manifest
            .command
            .split_first()
            .ok_or_else(|| ModuleError::Internal("manifest has no command".into()))?;
        let program = if prog.contains('/') {
            self.dir.join(prog)
        } else {
            PathBuf::from(prog)
        };
        let mut cmd = Command::new(program);
        cmd.args(args)
            .current_dir(&self.dir)
            .envs(&self.env)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        Ok(cmd)
    }

    fn spawn(
        &self,
    ) -> Result<(Child, ChildStdin, BufReader<tokio::process::ChildStdout>), ModuleError> {
        let mut child = self.command()?.spawn().map_err(|e| {
            ModuleError::Internal(format!(
                "can't start `{}`: {e}",
                self.manifest.command.join(" ")
            ))
        })?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| ModuleError::Internal("no stdin".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| ModuleError::Internal("no stdout".into()))?;
        if let Some(stderr) = child.stderr.take() {
            let name = self.manifest.name.clone();
            let redact = self.redact.clone();
            tokio::spawn(async move {
                let mut lines = BufReader::new(stderr).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    let mut line = line;
                    for s in &redact {
                        line = line.replace(s.as_str(), "[redacted]");
                    }
                    tracing::debug!(target: "spektr::module", "{name}: {line}");
                }
            });
        }
        Ok((child, stdin, BufReader::new(stdout)))
    }

    fn run_message(&self, input: &JobInput, cx: &ModuleCx) -> Result<HostMessage, ModuleError> {
        let deadline = jiff::Timestamp::now() + self.manifest.timeout_duration();
        Ok(HostMessage::Run {
            job: wire_id(input.job),
            input: Box::new(InputEntity {
                ty: input.entity.ty.to_string(),
                key: input.entity.key.clone(),
                value: input.entity.value.clone(),
                attrs: serde_json::to_value(&input.entity.attrs)
                    .map_err(|e| ModuleError::Internal(e.to_string()))?,
                blob_b64: input
                    .blob
                    .as_ref()
                    .map(|b| base64::engine::general_purpose::STANDARD.encode(b)),
            }),
            config: cx.config.clone(),
            secrets: cx.secrets.0.clone(),
            deadline: deadline.to_string(),
        })
    }

    async fn run_once(&self, input: JobInput, cx: ModuleCx) -> Result<(), ModuleError> {
        let (mut child, mut stdin, mut stdout) = self.spawn()?;
        write_msg(
            &mut stdin,
            &HostMessage::Hello {
                protocol: PROTOCOL_VERSION,
                spektr: env!("CARGO_PKG_VERSION").into(),
            },
        )
        .await?;
        write_msg(&mut stdin, &self.run_message(&input, &cx)?).await?;
        let job = wire_id(input.job);
        let outcome: Option<Result<(), ModuleError>> = loop {
            let line = tokio::select! {
                l = read_line(&mut stdout) => l?,
                _ = cx.cancel.cancelled() => {
                    let _ = write_msg(&mut stdin, &HostMessage::Cancel { job: job.clone() }).await;
                    finish(child, Some(stdin)).await;
                    return Err(ModuleError::Cancelled);
                }
            };
            let Some(line) = line else { break None };
            match parse(&line)? {
                ModuleMessage::Done { .. } => break Some(Ok(())),
                ModuleMessage::Error {
                    code,
                    message,
                    retryable,
                    retry_after,
                    ..
                } => break Some(Err(error(code, message, retryable, retry_after))),
                ModuleMessage::Ready => {}
                msg => {
                    if let Err(e) = cx.emit.emit(msg).await {
                        finish(child, Some(stdin)).await;
                        return Err(e);
                    }
                }
            }
        };
        match outcome {
            Some(result) => {
                finish(child, Some(stdin)).await;
                result
            }
            None => {
                drop(stdin);
                let status = tokio::time::timeout(EXIT_GRACE, child.wait()).await;
                match status {
                    Ok(Ok(s)) => Err(ModuleError::Internal(format!(
                        "exited ({s}) without sending done"
                    ))),
                    _ => {
                        let _ = child.kill().await;
                        Err(ModuleError::Internal(
                            "closed stdout without sending done".into(),
                        ))
                    }
                }
            }
        }
    }

    async fn serve_proc(&self) -> Result<Arc<ServeProc>, ModuleError> {
        let mut guard = self.serve.lock().await;
        if let Some(p) = guard.as_ref()
            && p.alive()
        {
            return Ok(Arc::clone(p));
        }
        let (child, mut stdin, mut stdout) = self.spawn()?;
        write_msg(
            &mut stdin,
            &HostMessage::Hello {
                protocol: PROTOCOL_VERSION,
                spektr: env!("CARGO_PKG_VERSION").into(),
            },
        )
        .await?;
        let ready = tokio::time::timeout(Duration::from_secs(10), read_line(&mut stdout))
            .await
            .map_err(|_| ModuleError::Internal("no `ready` within 10 s".into()))??;
        match ready.as_deref().map(parse) {
            Some(Ok(ModuleMessage::Ready)) => {}
            other => {
                return Err(ModuleError::Internal(format!(
                    "expected `ready`, got {other:?}"
                )));
            }
        }
        let proc = Arc::new(ServeProc {
            stdin: Mutex::new(stdin),
            routes: Arc::new(std::sync::Mutex::new(HashMap::new())),
            _child: Mutex::new(child),
            alive: Arc::new(std::sync::atomic::AtomicBool::new(true)),
        });
        let routes = Arc::clone(&proc.routes);
        let alive = Arc::clone(&proc.alive);
        let name = self.manifest.name.clone();
        tokio::spawn(async move {
            while let Ok(Some(line)) = read_line(&mut stdout).await {
                match parse(&line) {
                    Ok(msg) => {
                        let job = msg.job().map(str::to_string);
                        let tx = job.and_then(|j| {
                            routes
                                .lock()
                                .unwrap_or_else(|e| e.into_inner())
                                .get(&j)
                                .cloned()
                        });
                        match tx {
                            Some(tx) => {
                                let _ = tx.send(msg);
                            }
                            None => {
                                tracing::warn!("{name}: message for no running job: {}", msg.kind())
                            }
                        }
                    }
                    Err(e) => tracing::warn!("{name}: {e}"),
                }
            }
            alive.store(false, std::sync::atomic::Ordering::SeqCst);
            let mut routes = routes.lock().unwrap_or_else(|e| e.into_inner());
            for (job, tx) in routes.drain() {
                let _ = tx.send(ModuleMessage::Error {
                    job: Some(job),
                    code: ErrorCode::Internal,
                    message: "module process exited".into(),
                    retryable: false,
                    retry_after: None,
                });
            }
        });
        *guard = Some(Arc::clone(&proc));
        Ok(proc)
    }

    async fn run_serve(&self, input: JobInput, cx: ModuleCx) -> Result<(), ModuleError> {
        let proc = self.serve_proc().await?;
        let job = wire_id(input.job);
        let (tx, mut rx) = mpsc::unbounded_channel();
        proc.routes
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(job.clone(), tx);
        let result = async {
            proc.send(&self.run_message(&input, &cx)?).await?;
            loop {
                let msg = tokio::select! {
                    m = rx.recv() => m,
                    _ = cx.cancel.cancelled() => {
                        let _ = proc.send(&HostMessage::Cancel { job: job.clone() }).await;
                        return Err(ModuleError::Cancelled);
                    }
                };
                match msg {
                    None => return Err(ModuleError::Internal("module process exited".into())),
                    Some(ModuleMessage::Done { .. }) => return Ok(()),
                    Some(ModuleMessage::Error {
                        code,
                        message,
                        retryable,
                        retry_after,
                        ..
                    }) => return Err(error(code, message, retryable, retry_after)),
                    Some(ModuleMessage::Ready) => {}
                    Some(msg) => cx.emit.emit(msg).await?,
                }
            }
        }
        .await;
        proc.routes
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&job);
        result
    }
}

impl Module for ExecModule {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn run<'a>(&'a self, input: JobInput, cx: ModuleCx) -> BoxFuture<'a, Result<(), ModuleError>> {
        Box::pin(async move {
            if self.manifest.serve {
                self.run_serve(input, cx).await
            } else {
                self.run_once(input, cx).await
            }
        })
    }
}

struct ServeProc {
    stdin: Mutex<ChildStdin>,
    routes: Arc<std::sync::Mutex<HashMap<String, mpsc::UnboundedSender<ModuleMessage>>>>,
    _child: Mutex<Child>,
    alive: Arc<std::sync::atomic::AtomicBool>,
}

impl ServeProc {
    fn alive(&self) -> bool {
        self.alive.load(std::sync::atomic::Ordering::SeqCst)
    }

    async fn send(&self, m: &HostMessage) -> Result<(), ModuleError> {
        let mut stdin = self.stdin.lock().await;
        write_msg(&mut stdin, m).await
    }
}

fn wire_id(job: JobId) -> String {
    format!("j_{}", job.0.simple())
}

fn error(
    code: ErrorCode,
    message: String,
    retryable: bool,
    retry_after: Option<String>,
) -> ModuleError {
    ModuleError::Code {
        code,
        message,
        retryable,
        retry_after: retry_after.as_deref().and_then(parse_duration),
    }
}

fn parse(line: &str) -> Result<ModuleMessage, ModuleError> {
    serde_json::from_str(line).map_err(|e| {
        let shown: String = line.chars().take(120).collect();
        ModuleError::code(
            ErrorCode::Parse,
            format!("bad protocol line ({e}): {shown}"),
        )
    })
}

async fn write_msg(stdin: &mut ChildStdin, m: &HostMessage) -> Result<(), ModuleError> {
    let mut line = serde_json::to_vec(m).map_err(|e| ModuleError::Internal(e.to_string()))?;
    line.push(b'\n');
    stdin
        .write_all(&line)
        .await
        .map_err(|e| ModuleError::Internal(format!("writing to the module: {e}")))?;
    stdin
        .flush()
        .await
        .map_err(|e| ModuleError::Internal(format!("writing to the module: {e}")))
}

/// One line, at most [`MAX_LINE`] bytes; `None` at end of stream.
async fn read_line<R: AsyncBufRead + Unpin>(r: &mut R) -> Result<Option<String>, ModuleError> {
    let mut buf = Vec::new();
    let limit = u64::try_from(MAX_LINE).unwrap_or(u64::MAX) + 1;
    let n = (&mut *r)
        .take(limit)
        .read_until(b'\n', &mut buf)
        .await
        .map_err(|e| ModuleError::Internal(format!("reading from the module: {e}")))?;
    if n == 0 {
        return Ok(None);
    }
    if buf.len() > MAX_LINE {
        return Err(ModuleError::LimitExceeded(format!(
            "a line longer than {MAX_LINE} bytes"
        )));
    }
    while buf.last().is_some_and(|b| *b == b'\n' || *b == b'\r') {
        buf.pop();
    }
    String::from_utf8(buf)
        .map(Some)
        .map_err(|_| ModuleError::code(ErrorCode::Parse, "a line that is not UTF-8"))
}

/// Close stdin and give the process [`EXIT_GRACE`] to exit before killing it.
async fn finish(mut child: Child, stdin: Option<ChildStdin>) {
    drop(stdin);
    if tokio::time::timeout(EXIT_GRACE, child.wait())
        .await
        .is_err()
    {
        let _ = child.kill().await;
    }
}
