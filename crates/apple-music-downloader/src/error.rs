use std::io;
use std::net::AddrParseError;

use apple_music_api::AppleMusicApiError;
use apple_music_decryptor::AppleMusicDecryptorError;
use thiserror::Error;

pub type AppResult<T> = Result<T, AppError>;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("{0}")]
    Message(String),
    #[error("{0}")]
    Command(String),
    #[error("io error: {0}")]
    Io(#[from] io::Error),
    #[error("address parse error: {0}")]
    Address(#[from] AddrParseError),
    #[error("protocol error: {0}")]
    Protocol(String),
    #[error("native error: {0}")]
    Native(String),
    #[error("invalid device info: {0}")]
    InvalidDeviceInfo(String),
    #[error("{message}")]
    UpstreamHttp {
        status: reqwest::StatusCode,
        message: String,
        retry_after: Option<String>,
    },
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

impl From<AppleMusicApiError> for AppError {
    fn from(error: AppleMusicApiError) -> Self {
        match error {
            AppleMusicApiError::Message(message) => Self::Message(message),
            AppleMusicApiError::Http(error) => Self::Message(error.to_string()),
            AppleMusicApiError::Json(error) => Self::Message(error.to_string()),
            AppleMusicApiError::Xml(error) => Self::Message(error.to_string()),
            AppleMusicApiError::Protocol(message) => Self::Protocol(message),
            AppleMusicApiError::UpstreamHttp {
                status,
                message,
                retry_after,
            } => Self::UpstreamHttp {
                status,
                message,
                retry_after,
            },
        }
    }
}

impl From<AppleMusicDecryptorError> for AppError {
    fn from(error: AppleMusicDecryptorError) -> Self {
        match error {
            AppleMusicDecryptorError::Message(message) => Self::Message(message),
            AppleMusicDecryptorError::Command(message) => Self::Command(message),
            AppleMusicDecryptorError::Io(error) => Self::Io(error),
            AppleMusicDecryptorError::Json(error) => Self::Message(error.to_string()),
            AppleMusicDecryptorError::Http(error) => Self::Message(error.to_string()),
            AppleMusicDecryptorError::Mp4(error) => Self::Message(error.to_string()),
            AppleMusicDecryptorError::Library(error) => Self::Message(error.to_string()),
            AppleMusicDecryptorError::InvalidDeviceInfo(message) => {
                Self::InvalidDeviceInfo(message)
            }
            AppleMusicDecryptorError::Protocol(message) => Self::Protocol(message),
            AppleMusicDecryptorError::Native(message) => Self::Native(message),
            AppleMusicDecryptorError::UnexpectedTwoFactor => Self::UnexpectedTwoFactor,
        }
    }
}
