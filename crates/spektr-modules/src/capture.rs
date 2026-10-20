//! Web capture (MODULES §6): one GET, saving the HTML, headers, final URL
//! and status, plus a screenshot from Chrome or Chromium run headless as a
//! subprocess. The request goes through `net`, so it records and replays
//! like any module's; replay never starts a browser.

use std::path::{Path, PathBuf};
use std::time::Duration;

use futures::future::BoxFuture;
use spektr_engine::{CaptureError, CaptureRequest, CaptureService, Captured, MAX_CAPTURE};

use crate::net::{Net, Transport};

/// Screenshot window (MODULES §6).
const WINDOW: &str = "1280,2400";
const SCREENSHOT_TIMEOUT: Duration = Duration::from_secs(45);

/// Common install paths, after `PATH`.
const MAC_PATHS: &[&str] = &[
    "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
    "/Applications/Chromium.app/Contents/MacOS/Chromium",
    "/Applications/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing",
];
const BINARIES: &[&str] = &[
    "chrome-headless-shell",
    "google-chrome",
    "google-chrome-stable",
    "chromium",
    "chromium-browser",
    "chrome",
];

/// Find a browser: the copy `spektr setup capture` downloaded into the data
/// dir first, then `PATH`, then common install paths.
pub fn find_browser(data_dir: Option<&Path>) -> Option<PathBuf> {
    if let Some(d) = data_dir {
        let root = d.join("chrome");
        if let Some(p) = find_in(&root) {
            return Some(p);
        }
    }
    BINARIES
        .iter()
        .find_map(|b| which::which(b).ok())
        .or_else(|| MAC_PATHS.iter().map(PathBuf::from).find(|p| p.exists()))
}

/// The headless-shell binary anywhere under `root` (the extracted archive).
fn find_in(root: &Path) -> Option<PathBuf> {
    let entries = std::fs::read_dir(root).ok()?;
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            if let Some(f) = find_in(&p) {
                return Some(f);
            }
        } else if p.file_name().is_some_and(|n| n == "chrome-headless-shell") {
            return Some(p);
        }
    }
    None
}

pub struct Capture {
    net: Net,
    browser: Option<PathBuf>,
}

impl Capture {
    pub fn new(net: Net, data_dir: Option<&Path>) -> Self {
        let browser = match net.transport() {
            Transport::Replay(_) => None,
            _ => find_browser(data_dir),
        };
        Self { net, browser }
    }

    async fn screenshot(&self, url: &str) -> Result<Vec<u8>, String> {
        let Some(bin) = &self.browser else {
            return Err("no Chrome or Chromium found (spektr setup capture)".into());
        };
        let dir = tempdir()?;
        let out = dir.join("shot.png");
        let mut cmd = tokio::process::Command::new(bin);
        cmd.arg("--headless")
            .arg("--disable-gpu")
            .arg("--hide-scrollbars")
            .arg("--no-first-run")
            .arg("--mute-audio")
            .arg(format!("--user-data-dir={}", dir.join("profile").display()))
            .arg(format!("--window-size={WINDOW}"))
            .arg(format!("--screenshot={}", out.display()))
            .arg(url)
            .kill_on_drop(true)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        let status = tokio::time::timeout(SCREENSHOT_TIMEOUT, cmd.status())
            .await
            .map_err(|_| "the browser took too long".to_string())?
            .map_err(|e| format!("starting the browser: {e}"))?;
        let bytes = std::fs::read(&out)
            .map_err(|_| format!("the browser wrote no screenshot ({status})"))?;
        let _ = std::fs::remove_dir_all(&dir);
        Ok(bytes)
    }
}

fn tempdir() -> Result<PathBuf, String> {
    let d = std::env::temp_dir().join(format!(
        "spektr-capture-{}-{}",
        std::process::id(),
        jiff::Timestamp::now().as_nanosecond()
    ));
    std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
    Ok(d)
}

impl CaptureService for Capture {
    fn capture(&self, req: CaptureRequest) -> BoxFuture<'_, Result<Captured, CaptureError>> {
        Box::pin(async move {
            let resp = self
                .net
                .http
                .request("capture", "GET", &req.url, &[("accept", "text/html,*/*")])
                .await
                .map_err(|e| CaptureError::Failed(e.to_string()))?;
            if resp.body.len() as u64 > MAX_CAPTURE {
                return Err(CaptureError::TooLarge {
                    size: resp.body.len() as u64,
                });
            }
            let (screenshot, screenshot_note) = if !req.screenshot {
                (None, Some("not asked for".into()))
            } else if matches!(self.net.transport(), Transport::Replay(_)) {
                (None, Some("replay: no browser".into()))
            } else {
                match self.screenshot(&resp.final_url).await {
                    Ok(png) => (Some(png), None),
                    Err(e) => (None, Some(e)),
                }
            };
            let mut headers: Vec<(String, String)> = resp
                .headers
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            headers.sort();
            Ok(Captured {
                final_url: resp.final_url.clone(),
                status: resp.status,
                headers,
                html: resp.body.clone(),
                screenshot,
                screenshot_note,
            })
        })
    }

    fn screenshots(&self) -> bool {
        self.browser.is_some()
    }
}
