use std::path::PathBuf;

/// Errors from the case file.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("database: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("I/O: {0}")]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Normalize(#[from] spektr_model::NormalizeError),
    #[error("{0} is not a spektr case file")]
    NotACase(PathBuf),
    #[error(
        "{path} was written by a newer spektr (schema {found}, this build supports {supported}); upgrade spektr to open it"
    )]
    NewerSchema {
        path: PathBuf,
        found: i64,
        supported: i64,
    },
    #[error("{path} failed its integrity check ({detail}); run `spektr verify {path}`")]
    Corrupt { path: PathBuf, detail: String },
    #[error("{0} already exists")]
    Exists(PathBuf),
    #[error("{0} not found")]
    NotFound(String),
    #[error("{0}")]
    Invalid(String),
    #[error("blob of {size} bytes exceeds the {max}-byte limit")]
    BlobTooLarge { size: usize, max: usize },
    #[error("the store has shut down")]
    Closed,
}

pub type StoreResult<T> = Result<T, StoreError>;

pub(crate) fn invalid(msg: impl Into<String>) -> StoreError {
    StoreError::Invalid(msg.into())
}
