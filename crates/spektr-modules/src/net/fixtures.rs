//! Record/replay fixtures (MODULES §10). One JSON object per line:
//!
//! - HTTP: `{kind:"http", method, url, status, headers, body_b64, final_url?}`.
//!   Hand-written fixtures may give `body` instead: a string is the body
//!   text, any other JSON value is serialized as the (JSON) body.
//! - DNS:  `{kind:"dns", name, rtype, answers, rcode?}`
//!
//! Replay matches HTTP by method plus normalized URL, and DNS by name plus
//! record type. A request with no match fails.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Exchange {
    Http {
        method: String,
        url: String,
        status: u16,
        #[serde(default)]
        headers: BTreeMap<String, String>,
        #[serde(default)]
        body_b64: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        final_url: Option<String>,
    },
    Dns {
        name: String,
        rtype: String,
        #[serde(default)]
        answers: Vec<String>,
        #[serde(default = "noerror")]
        rcode: String,
    },
}

fn noerror() -> String {
    "NOERROR".into()
}

/// Normalized request URL: the WHATWG serialization (lowercase scheme and
/// host, default port dropped), fragment removed.
pub fn normalize_url(u: &str) -> String {
    match url::Url::parse(u) {
        Ok(mut p) => {
            p.set_fragment(None);
            p.to_string()
        }
        Err(_) => u.to_string(),
    }
}

pub(crate) fn dns_key(name: &str, rtype: &str) -> (String, String) {
    (
        name.trim_end_matches('.').to_ascii_lowercase(),
        rtype.to_ascii_uppercase(),
    )
}

#[derive(Debug, Default)]
pub struct Fixtures {
    http: HashMap<(String, String), Exchange>,
    dns: HashMap<(String, String), Exchange>,
    misses: Mutex<Vec<String>>,
}

#[derive(Debug, thiserror::Error)]
pub enum FixtureError {
    #[error("{file}:{line}: {msg}")]
    Parse {
        file: String,
        line: usize,
        msg: String,
    },
    #[error("{0}: {1}")]
    Io(String, std::io::Error),
}

impl Fixtures {
    pub fn len(&self) -> usize {
        self.http.len() + self.dns.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Add JSON lines. Two fixtures answering the same request differently
    /// are an error, so a response can't be silently overridden.
    pub fn add_jsonl(&mut self, name: &str, text: &str) -> Result<(), FixtureError> {
        for (i, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with("//") {
                continue;
            }
            let err = |e: serde_json::Error| FixtureError::Parse {
                file: name.into(),
                line: i + 1,
                msg: e.to_string(),
            };
            let mut v: serde_json::Value = serde_json::from_str(line).map_err(err)?;
            if let Some(obj) = v.as_object_mut()
                && let Some(body) = obj.remove("body")
            {
                use base64::Engine as _;
                let bytes = match body {
                    serde_json::Value::String(s) => s.into_bytes(),
                    other => serde_json::to_vec(&other).map_err(err)?,
                };
                obj.insert(
                    "body_b64".into(),
                    base64::engine::general_purpose::STANDARD
                        .encode(bytes)
                        .into(),
                );
            }
            let ex: Exchange = serde_json::from_value(v).map_err(err)?;
            if let Some(prev) = self.get(&ex)
                && *prev != ex
            {
                return Err(FixtureError::Parse {
                    file: name.into(),
                    line: i + 1,
                    msg: format!(
                        "{} is already answered differently by another fixture",
                        describe(&ex)
                    ),
                });
            }
            self.insert(ex);
        }
        Ok(())
    }

    fn get(&self, ex: &Exchange) -> Option<&Exchange> {
        match ex {
            Exchange::Http { method, url, .. } => self
                .http
                .get(&(method.to_ascii_uppercase(), normalize_url(url))),
            Exchange::Dns { name, rtype, .. } => self.dns.get(&dns_key(name, rtype)),
        }
    }

    pub fn insert(&mut self, ex: Exchange) {
        match &ex {
            Exchange::Http { method, url, .. } => {
                self.http
                    .insert((method.to_ascii_uppercase(), normalize_url(url)), ex);
            }
            Exchange::Dns { name, rtype, .. } => {
                self.dns.insert(dns_key(name, rtype), ex);
            }
        }
    }

    /// Every `*.jsonl` under `dir`, recursively, in path order.
    pub fn load_dir(dir: &Path) -> Result<Self, FixtureError> {
        let mut f = Self::default();
        let mut files = Vec::new();
        collect(dir, &mut files).map_err(|e| FixtureError::Io(dir.display().to_string(), e))?;
        files.sort();
        for p in files {
            let text = std::fs::read_to_string(&p)
                .map_err(|e| FixtureError::Io(p.display().to_string(), e))?;
            f.add_jsonl(&p.display().to_string(), &text)?;
        }
        Ok(f)
    }

    pub fn http(&self, method: &str, url: &str) -> Option<&Exchange> {
        let key = (method.to_ascii_uppercase(), normalize_url(url));
        let hit = self.http.get(&key);
        if hit.is_none() {
            self.miss(format!("{} {}", key.0, key.1));
        }
        hit
    }

    pub fn dns(&self, name: &str, rtype: &str) -> Option<&Exchange> {
        let key = dns_key(name, rtype);
        let hit = self.dns.get(&key);
        if hit.is_none() {
            self.miss(format!("DNS {} {}", key.1, key.0));
        }
        hit
    }

    fn miss(&self, what: String) {
        self.misses
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(what);
    }

    pub fn misses(&self) -> Vec<String> {
        self.misses
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
}

fn describe(ex: &Exchange) -> String {
    match ex {
        Exchange::Http { method, url, .. } => format!("{method} {}", normalize_url(url)),
        Exchange::Dns { name, rtype, .. } => format!("DNS {rtype} {name}"),
    }
}

fn collect(dir: &Path, out: &mut Vec<std::path::PathBuf>) -> std::io::Result<()> {
    for e in std::fs::read_dir(dir)? {
        let p = e?.path();
        if p.is_dir() {
            collect(&p, out)?;
        } else if p.extension().is_some_and(|x| x == "jsonl") {
            out.push(p);
        }
    }
    Ok(())
}

/// Append an exchange to `<dir>/<module>/recorded.jsonl`.
pub(crate) fn record(dir: &Path, module: &str, ex: &Exchange) -> Result<(), String> {
    use std::io::Write;
    let d = dir.join(module);
    std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(d.join("recorded.jsonl"))
        .map_err(|e| e.to_string())?;
    let line = serde_json::to_string(ex).map_err(|e| e.to_string())?;
    writeln!(f, "{line}").map_err(|e| e.to_string())
}
