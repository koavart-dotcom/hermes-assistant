use thiserror::Error;

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("zip error: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("keyring error: {0}")]
    Keyring(String),
    #[error("installer protocol error: {0}")]
    Protocol(String),
    #[error("stage `{stage}` failed: {reason}")]
    StageFailed { stage: String, reason: String },
    #[error("process `{0}` exited with status {1}")]
    ProcessFailed(String, String),
    #[error("cancelled")]
    Cancelled,
    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, CoreError>;
