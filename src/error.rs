use std::io;
use std::net::AddrParseError;

use thiserror::Error;

pub type AppResult<T> = Result<T, AppError>;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("{0}")]
    Message(String),
    #[error("io error: {0}")]
    Io(#[from] io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("http client error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("xml error: {0}")]
    Xml(#[from] roxmltree::Error),
    #[error("mp4 error: {0}")]
    Mp4(#[from] crate::daemon::mp4::Mp4Error),
    #[error("libloading error: {0}")]
    Library(#[from] libloading::Error),
    #[error("address parse error: {0}")]
    Address(#[from] AddrParseError),
    #[error("invalid device info: {0}")]
    InvalidDeviceInfo(String),
    #[error("protocol error: {0}")]
    Protocol(String),
    #[error("native error: {0}")]
    Native(String),
    #[error("no active session")]
    NoActiveSession,
    #[error("2FA was not requested for the current login flow")]
    UnexpectedTwoFactor,
}

impl From<&str> for AppError {
    fn from(value: &str) -> Self {
        Self::Message(value.to_owned())
    }
}
