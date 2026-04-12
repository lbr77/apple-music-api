use thiserror::Error;

pub type ApiResult<T> = Result<T, AppleMusicApiError>;

#[derive(Debug, Error)]
pub enum AppleMusicApiError {
    #[error("{0}")]
    Message(String),
    #[error("http client error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("xml error: {0}")]
    Xml(#[from] roxmltree::Error),
    #[error("protocol error: {0}")]
    Protocol(String),
    #[error("{message}")]
    UpstreamHttp {
        status: reqwest::StatusCode,
        message: String,
        retry_after: Option<String>,
    },
}

impl From<&str> for AppleMusicApiError {
    fn from(value: &str) -> Self {
        Self::Message(value.to_owned())
    }
}
