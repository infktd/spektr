//! Web capture is a service, not a module (MODULES §6): the binary injects
//! an implementation from `spektr-modules::capture`.

use futures::future::BoxFuture;
use spektr_model::EntityId;

use crate::module::Emitter;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureRequest {
    pub url: String,
    /// The entity the capture is evidence for.
    pub entity: EntityId,
    pub screenshot: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CaptureError {
    #[error("{0}")]
    Failed(String),
    #[error("capture of {size} bytes exceeds the 25 MB limit")]
    TooLarge { size: u64 },
}

pub trait CaptureService: Send + Sync + 'static {
    fn capture<'a>(
        &'a self,
        req: CaptureRequest,
        emit: Emitter,
    ) -> BoxFuture<'a, Result<(), CaptureError>>;
}
