//! Web capture is a service, not a module (MODULES §6): the binary injects
//! an implementation from `spektr-modules::capture`. The service fetches;
//! the engine stores what it brings back as a `capture` observation (HTML
//! as its blob, the screenshot and headers linked in its summary).

use futures::future::BoxFuture;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureRequest {
    pub url: String,
    pub screenshot: bool,
}

/// What one capture brought back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Captured {
    pub final_url: String,
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub html: Vec<u8>,
    /// PNG, when a browser was available and a screenshot was asked for.
    pub screenshot: Option<Vec<u8>>,
    /// Why there's no screenshot, when one was asked for.
    pub screenshot_note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CaptureError {
    #[error("{0}")]
    Failed(String),
    #[error("capture of {size} bytes exceeds the 25 MB limit")]
    TooLarge { size: u64 },
}

/// The 25 MB capture limit (DATA_MODEL §10).
pub const MAX_CAPTURE: u64 = 25 * 1024 * 1024;

pub trait CaptureService: Send + Sync + 'static {
    fn capture(&self, req: CaptureRequest) -> BoxFuture<'_, Result<Captured, CaptureError>>;
    /// Whether screenshots are possible (a browser was found).
    fn screenshots(&self) -> bool;
}
