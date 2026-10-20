//! Shared network access for every built-in module and for capture
//! (MODULES §4, §10): HTTP and DNS behind one `Transport`, with per-host
//! rate limiting, a user agent, and record/replay so tests, the tutor and
//! QA never touch the network.

mod dns;
mod fixtures;
mod http;

use std::path::PathBuf;
use std::sync::{Arc, Once};

pub use dns::{Dns, DnsConfig, DnsResponse, RType, parse_resolv_conf, reverse_name};
pub use fixtures::{Exchange, Fixtures, normalize_url};
pub use http::{Http, HttpResponse, ModuleHttp};

/// Where requests go.
#[derive(Clone)]
pub enum Transport {
    /// The real network.
    Live,
    /// The real network, and every exchange appended to `<dir>/<module>/recorded.jsonl`.
    Record(PathBuf),
    /// Only recorded fixtures. A request with no match is an error.
    Replay(Arc<Fixtures>),
}

impl std::fmt::Debug for Transport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Transport::Live => f.write_str("Live"),
            Transport::Record(d) => write!(f, "Record({})", d.display()),
            Transport::Replay(fx) => write!(f, "Replay({} exchanges)", fx.len()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NetError {
    #[error("no fixture for {method} {url}")]
    NoFixture { method: String, url: String },
    #[error("no fixture for DNS {rtype} {name}")]
    NoDnsFixture { name: String, rtype: String },
    #[error("{0}")]
    Http(String),
    #[error("timed out")]
    Timeout,
    #[error("response larger than {0} bytes")]
    TooLarge(usize),
    #[error("DNS: {0}")]
    Dns(String),
    #[error("no DNS nameservers (set [dns] nameservers in config.toml)")]
    NoNameservers,
    #[error("bad URL `{0}`")]
    BadUrl(String),
    #[error("recording: {0}")]
    Record(String),
}

impl NetError {
    /// Missing fixtures are test failures, not transient errors.
    pub fn is_fixture_miss(&self) -> bool {
        matches!(
            self,
            NetError::NoFixture { .. } | NetError::NoDnsFixture { .. }
        )
    }
}

/// HTTP and DNS for modules. Cheap to clone.
#[derive(Clone)]
pub struct Net {
    pub http: Http,
    pub dns: Dns,
    transport: Transport,
}

impl std::fmt::Debug for Net {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Net")
            .field("transport", &self.transport)
            .finish()
    }
}

static PROVIDER: Once = Once::new();

/// Install rustls' ring provider once per process (ARCHITECTURE §6, rule 1).
pub fn install_crypto_provider() {
    PROVIDER.call_once(|| {
        // Err means another provider is already installed, which is fine.
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

impl Net {
    pub fn new(transport: Transport, dns: DnsConfig) -> Result<Self, NetError> {
        install_crypto_provider();
        Ok(Self {
            http: Http::new(transport.clone())?,
            dns: Dns::new(transport.clone(), dns),
            transport,
        })
    }

    /// Replay only, from fixtures.
    pub fn replay(fixtures: Fixtures) -> Self {
        let t = Transport::Replay(Arc::new(fixtures));
        // Building a replay client can't fail: no TLS, no sockets.
        Self {
            http: Http::replay(t.clone()),
            dns: Dns::new(t.clone(), DnsConfig::default()),
            transport: t,
        }
    }

    pub fn transport(&self) -> &Transport {
        &self.transport
    }

    /// Requests that found no fixture (replay only), for tests.
    pub fn misses(&self) -> Vec<String> {
        match &self.transport {
            Transport::Replay(f) => f.misses(),
            _ => Vec::new(),
        }
    }
}
