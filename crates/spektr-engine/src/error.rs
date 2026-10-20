use spektr_store::StoreError;

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error(transparent)]
    Normalize(#[from] spektr_model::NormalizeError),
    #[error(transparent)]
    Vocab(#[from] spektr_model::VocabError),
    #[error("unknown module `{0}`")]
    UnknownModule(String),
    #[error("{0}")]
    Ineligible(String),
    #[error("{0}")]
    Invalid(String),
    #[error("rules profile: {0}")]
    Profile(String),
    #[error("timed out after {0:?} waiting for the engine to go idle")]
    Timeout(std::time::Duration),
    #[error("the engine has shut down")]
    Closed,
    #[error("I/O: {0}")]
    Io(#[from] std::io::Error),
}

pub type EngineResult<T> = Result<T, EngineError>;
