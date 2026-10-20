//! Exec protocol v1 (MODULES §3): UTF-8 JSON, one object per line, over the
//! module's stdin and stdout. Native modules emit the same [`ModuleMessage`]s.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Host → module.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum HostMessage {
    Hello {
        protocol: u32,
        spektr: String,
    },
    Run {
        job: String,
        input: Box<InputEntity>,
        config: serde_json::Value,
        /// Only the secrets the manifest declares (required, plus optional
        /// ones that are set).
        secrets: BTreeMap<String, String>,
        /// RFC 3339.
        deadline: String,
    },
    Cancel {
        job: String,
    },
    Shutdown,
}

/// The job's input entity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct InputEntity {
    #[serde(rename = "type")]
    pub ty: String,
    pub key: String,
    pub value: String,
    #[serde(default)]
    pub attrs: serde_json::Value,
    /// When the input is an `image`: its bytes, base64 (at most 25 MB).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blob_b64: Option<String>,
}

/// Module → host.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ModuleMessage {
    /// Serve mode: the module is ready for `run` messages.
    Ready,
    /// Raw evidence. `raw` is UTF-8 text; use `raw_b64` for binary content.
    Observation {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        job: Option<String>,
        /// Local id, cited by later messages' `evidence`.
        id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source_url: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        content_type: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        raw: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        raw_b64: Option<String>,
        /// Small structured facts about this observation (counts, status).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        summary: Option<serde_json::Value>,
    },
    Entity {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        job: Option<String>,
        /// Local name for this entity, used by later `edge` / `attr`
        /// messages. `$input` is the job's input.
        #[serde(rename = "ref")]
        reference: String,
        entity: EntitySpec,
        evidence: Vec<String>,
    },
    Attr {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        job: Option<String>,
        /// An entity `ref` or `$input`.
        target: String,
        name: String,
        value: AttrValue,
        /// True: the value appears on the timeline.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        event: bool,
        evidence: Vec<String>,
    },
    Edge {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        job: Option<String>,
        src: String,
        predicate: String,
        dst: String,
        #[serde(default, skip_serializing_if = "serde_json::Map::is_empty")]
        attrs: serde_json::Map<String, serde_json::Value>,
        evidence: Vec<String>,
    },
    Review {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        job: Option<String>,
        /// `sweep_unsure` or `same_person`.
        kind: String,
        payload: serde_json::Value,
        evidence: Vec<String>,
    },
    Progress {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        job: Option<String>,
        pct: u8,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        note: Option<String>,
    },
    Log {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        job: Option<String>,
        level: LogLevel,
        msg: String,
    },
    Done {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        job: Option<String>,
    },
    Error {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        job: Option<String>,
        code: ErrorCode,
        message: String,
        #[serde(default)]
        retryable: bool,
        /// e.g. `32s`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        retry_after: Option<String>,
    },
}

impl ModuleMessage {
    pub fn job(&self) -> Option<&str> {
        match self {
            ModuleMessage::Ready => None,
            ModuleMessage::Observation { job, .. }
            | ModuleMessage::Entity { job, .. }
            | ModuleMessage::Attr { job, .. }
            | ModuleMessage::Edge { job, .. }
            | ModuleMessage::Review { job, .. }
            | ModuleMessage::Progress { job, .. }
            | ModuleMessage::Log { job, .. }
            | ModuleMessage::Done { job }
            | ModuleMessage::Error { job, .. } => job.as_deref(),
        }
    }

    /// Set the job id (serve-mode routing, native emitters).
    pub fn with_job(mut self, id: &str) -> Self {
        match &mut self {
            ModuleMessage::Ready => {}
            ModuleMessage::Observation { job, .. }
            | ModuleMessage::Entity { job, .. }
            | ModuleMessage::Attr { job, .. }
            | ModuleMessage::Edge { job, .. }
            | ModuleMessage::Review { job, .. }
            | ModuleMessage::Progress { job, .. }
            | ModuleMessage::Log { job, .. }
            | ModuleMessage::Done { job }
            | ModuleMessage::Error { job, .. } => *job = Some(id.to_string()),
        }
        self
    }

    /// `done` or `error`: nothing follows for this job.
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            ModuleMessage::Done { .. } | ModuleMessage::Error { .. }
        )
    }

    pub fn kind(&self) -> &'static str {
        match self {
            ModuleMessage::Ready => "ready",
            ModuleMessage::Observation { .. } => "observation",
            ModuleMessage::Entity { .. } => "entity",
            ModuleMessage::Attr { .. } => "attr",
            ModuleMessage::Edge { .. } => "edge",
            ModuleMessage::Review { .. } => "review",
            ModuleMessage::Progress { .. } => "progress",
            ModuleMessage::Log { .. } => "log",
            ModuleMessage::Done { .. } => "done",
            ModuleMessage::Error { .. } => "error",
        }
    }
}

/// An entity a module found.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct EntitySpec {
    #[serde(rename = "type")]
    pub ty: String,
    pub value: String,
    /// Image entities: the observation id holding the bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blob: Option<String>,
    /// Person/organization: a stable source id, so repeated runs update one
    /// entity (DATA_MODEL §1.3).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dedup_id: Option<String>,
    /// Certificate: `sha256:<DER fingerprint>` or `crtsh:<id>`, when the
    /// value is a display label.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
}

/// Attribute values, as in DATA_MODEL §2.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum AttrValue {
    Text(String),
    Int(i64),
    Bool(bool),
    /// RFC 3339.
    Time(String),
    /// `YYYY-MM-DD`.
    Date(String),
    Geo {
        lat: f64,
        lon: f64,
    },
    Url(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

/// Only `rate_limited` and `upstream` are retried automatically.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    RateLimited,
    NotFound,
    Auth,
    Upstream,
    Parse,
    Timeout,
    Internal,
}

impl ErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            ErrorCode::RateLimited => "rate_limited",
            ErrorCode::NotFound => "not_found",
            ErrorCode::Auth => "auth",
            ErrorCode::Upstream => "upstream",
            ErrorCode::Parse => "parse",
            ErrorCode::Timeout => "timeout",
            ErrorCode::Internal => "internal",
        }
    }

    pub fn auto_retry(self) -> bool {
        matches!(self, ErrorCode::RateLimited | ErrorCode::Upstream)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every line of the MODULES §3.3 example parses.
    #[test]
    fn spec_examples_parse() {
        let lines = [
            r#"{"type":"ready"}"#,
            r#"{"type":"observation","job":"j_1","id":"o1","source_url":"https://lemmy.world/api/v3/user?username=jmarlow","content_type":"application/json","raw":"{}"}"#,
            r#"{"type":"entity","job":"j_1","ref":"e1","entity":{"type":"account","value":"lemmy:jmarlow@lemmy.world"},"evidence":["o1"]}"#,
            r#"{"type":"attr","job":"j_1","target":"e1","name":"display_name","value":{"text":"Jordan M."},"evidence":["o1"]}"#,
            r#"{"type":"attr","job":"j_1","target":"e1","name":"last_active","value":{"time":"2026-10-16T09:12:00Z"},"event":true,"evidence":["o1"]}"#,
            r#"{"type":"edge","job":"j_1","src":"$input","predicate":"has_account","dst":"e1","evidence":["o1"]}"#,
            r#"{"type":"observation","job":"j_1","id":"o2","source_url":"https://files.example/avatar.png","content_type":"image/png","raw_b64":"iVBORw0="}"#,
            r#"{"type":"entity","job":"j_1","ref":"e2","entity":{"type":"image","blob":"o2","value":"avatar.png"},"evidence":["o2"]}"#,
            r#"{"type":"entity","job":"j_1","ref":"e3","entity":{"type":"organization","value":"Kestrel Labs LLC","dedup_id":"KL-1234"},"evidence":["o1"]}"#,
            r#"{"type":"review","job":"j_1","kind":"sweep_unsure","payload":{"site":"example-forum","url":"https://forum.example/u/x","reason":"login wall"},"evidence":["o1"]}"#,
            r#"{"type":"progress","job":"j_1","pct":60,"note":"120/300 sites"}"#,
            r#"{"type":"log","job":"j_1","level":"info","msg":"rate limited by host, backing off"}"#,
            r#"{"type":"done","job":"j_1"}"#,
            r#"{"type":"error","job":"j_1","code":"rate_limited","message":"429 from host","retryable":true,"retry_after":"32s"}"#,
        ];
        for line in lines {
            let m: ModuleMessage =
                serde_json::from_str(line).unwrap_or_else(|e| panic!("{line}: {e}"));
            // and round-trips
            let back: ModuleMessage =
                serde_json::from_str(&serde_json::to_string(&m).unwrap()).unwrap();
            assert_eq!(m, back);
        }
        let host = [
            r#"{"type":"hello","protocol":1,"spektr":"0.1.0"}"#,
            r#"{"type":"run","job":"j_1","input":{"type":"username","key":"jmarlow.wav","value":"jmarlow.wav","attrs":{}},"config":{"instances":["lemmy.world"]},"secrets":{},"deadline":"2026-10-20T18:06:12Z"}"#,
            r#"{"type":"cancel","job":"j_1"}"#,
            r#"{"type":"shutdown"}"#,
        ];
        for line in host {
            let _: HostMessage =
                serde_json::from_str(line).unwrap_or_else(|e| panic!("{line}: {e}"));
        }
    }

    #[test]
    fn strict_parsing() {
        assert!(serde_json::from_str::<ModuleMessage>(r#"{"type":"teleport"}"#).is_err());
        assert!(
            serde_json::from_str::<ModuleMessage>(r#"{"type":"done","job":"j","extra":1}"#)
                .is_err()
        );
        let m: ModuleMessage = serde_json::from_str(r#"{"type":"done"}"#).unwrap();
        assert_eq!(m.clone().with_job("j_9").job(), Some("j_9"));
        assert!(m.is_terminal());
    }
}
