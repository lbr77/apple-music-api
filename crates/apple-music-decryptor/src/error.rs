use std::io;

use thiserror::Error;

pub type AppResult<T> = Result<T, AppleMusicDecryptorError>;

#[derive(Debug, Error)]
pub enum AppleMusicDecryptorError {
    #[error("{0}")]
    Message(String),
    #[error("{0}")]
    Command(String),
    #[error("io error: {0}")]
    Io(#[from] io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("http client error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("mp4 error: {0}")]
    Mp4(#[from] crate::mp4::Mp4Error),
    #[error("libloading error: {0}")]
    Library(#[from] libloading::Error),
    #[error("invalid device info: {0}")]
    InvalidDeviceInfo(String),
    #[error("protocol error: {0}")]
    Protocol(String),
    #[error("native error: {0}")]
    Native(String),
    #[error("2FA was not requested for the current login flow")]
    UnexpectedTwoFactor,
}

impl From<&str> for AppleMusicDecryptorError {
    fn from(value: &str) -> Self {
        Self::Message(value.to_owned())
    }
}
