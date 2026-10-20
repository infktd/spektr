//! HTTP for modules: reqwest (rustls + ring) live, fixtures in replay,
//! per-host rate limiting with `governor` keyed by destination host.

use std::collections::BTreeMap;
use std::num::NonZeroU32;
use std::sync::Arc;
use std::time::Duration;

use base64::Engine as _;
use governor::{DefaultKeyedRateLimiter, Quota, RateLimiter};

use super::fixtures::{Exchange, record};
use super::{NetError, Transport};

/// Bodies larger than this fail the request.
pub const MAX_BODY: usize = 16 * 1024 * 1024;

pub const USER_AGENT: &str = concat!(
    "spektr/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/infktd/spektr)"
);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    pub status: u16,
    /// Lowercase header names.
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
    /// After redirects.
    pub final_url: String,
}

impl HttpResponse {
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }

    pub fn json<T: serde::de::DeserializeOwned>(&self) -> Result<T, serde_json::Error> {
        serde_json::from_slice(&self.body)
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .get(&name.to_ascii_lowercase())
            .map(String::as_str)
    }

    pub fn content_type(&self) -> Option<&str> {
        self.header("content-type")
    }
}

#[derive(Clone)]
pub struct Http {
    client: Option<reqwest::Client>,
    transport: Transport,
}

impl Http {
    pub(crate) fn new(transport: Transport) -> Result<Self, NetError> {
        if matches!(transport, Transport::Replay(_)) {
            return Ok(Self::replay(transport));
        }
        let client = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .timeout(Duration::from_secs(30))
            .connect_timeout(Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::limited(10))
            .build()
            .map_err(|e| NetError::Http(format!("building the HTTP client: {e}")))?;
        Ok(Self {
            client: Some(client),
            transport,
        })
    }

    pub(crate) fn replay(transport: Transport) -> Self {
        Self {
            client: None,
            transport,
        }
    }

    /// A handle for one module, rate limited per destination host by its
    /// manifest's `rate_limit` (`N/s` or `N/min`).
    pub fn for_module(&self, module: &str, rate_limit: Option<&str>) -> ModuleHttp {
        let limiter = rate_limit
            .and_then(spektr_protocol::parse_rate)
            .and_then(|(n, per)| {
                let n = NonZeroU32::new(n)?;
                Quota::with_period(per / n.get()).map(|q| q.allow_burst(n))
            })
            .map(|q| Arc::new(RateLimiter::keyed(q)));
        ModuleHttp {
            http: self.clone(),
            module: module.to_string(),
            limiter,
        }
    }

    pub async fn request(
        &self,
        module: &str,
        method: &str,
        url: &str,
        headers: &[(&str, &str)],
    ) -> Result<HttpResponse, NetError> {
        if let Transport::Replay(fx) = &self.transport {
            return match fx.http(method, url) {
                Some(Exchange::Http {
                    status,
                    headers,
                    body_b64,
                    final_url,
                    ..
                }) => Ok(HttpResponse {
                    status: *status,
                    headers: headers
                        .iter()
                        .map(|(k, v)| (k.to_ascii_lowercase(), v.clone()))
                        .collect(),
                    body: base64::engine::general_purpose::STANDARD
                        .decode(body_b64.as_bytes())
                        .map_err(|e| NetError::Http(format!("fixture body for {url}: {e}")))?,
                    final_url: final_url
                        .clone()
                        .unwrap_or_else(|| super::fixtures::normalize_url(url)),
                }),
                _ => Err(NetError::NoFixture {
                    method: method.to_ascii_uppercase(),
                    url: super::fixtures::normalize_url(url),
                }),
            };
        }
        let client = self
            .client
            .as_ref()
            .ok_or_else(|| NetError::Http("no HTTP client".into()))?;
        let m = reqwest::Method::from_bytes(method.as_bytes())
            .map_err(|_| NetError::Http(format!("bad method {method}")))?;
        let mut req = client.request(m, url);
        for (k, v) in headers {
            req = req.header(*k, *v);
        }
        let mut resp = req.send().await.map_err(|e| {
            if e.is_timeout() {
                NetError::Timeout
            } else {
                NetError::Http(e.to_string())
            }
        })?;
        let status = resp.status().as_u16();
        let final_url = resp.url().to_string();
        let resp_headers: BTreeMap<String, String> = resp
            .headers()
            .iter()
            .filter_map(|(k, v)| {
                v.to_str()
                    .ok()
                    .map(|v| (k.as_str().to_ascii_lowercase(), v.to_string()))
            })
            .collect();
        let mut body = Vec::new();
        while let Some(chunk) = resp.chunk().await.map_err(|e| {
            if e.is_timeout() {
                NetError::Timeout
            } else {
                NetError::Http(e.to_string())
            }
        })? {
            if body.len() + chunk.len() > MAX_BODY {
                return Err(NetError::TooLarge(MAX_BODY));
            }
            body.extend_from_slice(&chunk);
        }
        let out = HttpResponse {
            status,
            headers: resp_headers,
            body,
            final_url,
        };
        if let Transport::Record(dir) = &self.transport {
            let ex = Exchange::Http {
                method: method.to_ascii_uppercase(),
                url: url.to_string(),
                status,
                headers: out.headers.clone(),
                body_b64: base64::engine::general_purpose::STANDARD.encode(&out.body),
                final_url: (out.final_url != super::fixtures::normalize_url(url))
                    .then(|| out.final_url.clone()),
            };
            record(dir, module, &ex).map_err(NetError::Record)?;
        }
        Ok(out)
    }
}

/// One module's HTTP handle.
#[derive(Clone)]
pub struct ModuleHttp {
    http: Http,
    module: String,
    limiter: Option<Arc<DefaultKeyedRateLimiter<String>>>,
}

impl ModuleHttp {
    pub async fn get(&self, url: &str, headers: &[(&str, &str)]) -> Result<HttpResponse, NetError> {
        if let Some(l) = &self.limiter
            && !matches!(self.http.transport, Transport::Replay(_))
        {
            let host = url::Url::parse(url)
                .ok()
                .and_then(|u| u.host_str().map(str::to_string))
                .ok_or_else(|| NetError::BadUrl(url.to_string()))?;
            l.until_key_ready(&host).await;
        }
        self.http.request(&self.module, "GET", url, headers).await
    }
}
