//! Logs go to a daily rolling file in the state dir, never to the terminal
//! while the TUI runs (ARCHITECTURE §5). Known secret values are redacted
//! from every line.

use std::io::Write;
use std::sync::Arc;

use anyhow::Result;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt::MakeWriter;

use crate::paths::Paths;

#[derive(Clone)]
struct Redacting<W> {
    inner: W,
    secrets: Arc<Vec<String>>,
}

struct RedactingWriter<W: Write> {
    inner: W,
    secrets: Arc<Vec<String>>,
    buf: Vec<u8>,
}

impl<W: Write> Write for RedactingWriter<W> {
    fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
        self.buf.extend_from_slice(data);
        Ok(data.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        if self.buf.is_empty() {
            return self.inner.flush();
        }
        let mut text = String::from_utf8_lossy(&self.buf).into_owned();
        for s in self.secrets.iter() {
            text = text.replace(s.as_str(), "[redacted]");
        }
        self.buf.clear();
        self.inner.write_all(text.as_bytes())?;
        self.inner.flush()
    }
}

impl<W: Write> Drop for RedactingWriter<W> {
    fn drop(&mut self) {
        let _ = self.flush();
    }
}

impl<'a, M: MakeWriter<'a>> MakeWriter<'a> for Redacting<M> {
    type Writer = RedactingWriter<M::Writer>;

    fn make_writer(&'a self) -> Self::Writer {
        RedactingWriter {
            inner: self.inner.make_writer(),
            secrets: Arc::clone(&self.secrets),
            buf: Vec::new(),
        }
    }
}

/// Start file logging. Keep the guard alive for the life of the process.
pub fn init(
    paths: &Paths,
    secrets: Vec<String>,
) -> Result<tracing_appender::non_blocking::WorkerGuard> {
    std::fs::create_dir_all(paths.logs_dir())?;
    let appender = tracing_appender::rolling::daily(paths.logs_dir(), "spektr.log");
    let (writer, guard) = tracing_appender::non_blocking(appender);
    let filter = EnvFilter::try_from_env("SPEKTR_LOG").unwrap_or_else(|_| EnvFilter::new("info"));
    let secrets: Vec<String> = secrets.into_iter().filter(|s| s.len() >= 4).collect();
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_ansi(false)
        .with_writer(Redacting {
            inner: writer,
            secrets: Arc::new(secrets),
        })
        .try_init();
    Ok(guard)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_known_secrets() {
        let mut out = Vec::new();
        {
            let mut w = RedactingWriter {
                inner: &mut out,
                secrets: Arc::new(vec!["ghp_fictional123".into()]),
                buf: Vec::new(),
            };
            write!(w, "token=ghp_fictional123 ok").unwrap();
        }
        assert_eq!(String::from_utf8(out).unwrap(), "token=[redacted] ok");
    }
}
