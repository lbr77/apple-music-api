use axum::response::{IntoResponse, Response};

use crate::error::AppError;

use super::ResponseFormat;
use super::render::subsonic_error_response;

#[derive(Debug)]
pub(super) struct SubsonicError {
    pub(super) format: ResponseFormat,
    pub(super) code: i32,
    pub(super) message: String,
}

impl SubsonicError {
    pub(super) fn generic(format: ResponseFormat, message: impl Into<String>) -> Self {
        Self {
            format,
            code: 0,
            message: message.into(),
        }
    }

    pub(super) fn required_parameter(format: ResponseFormat, message: impl Into<String>) -> Self {
        Self {
            format,
            code: 10,
            message: message.into(),
        }
    }

    pub(super) fn authentication(format: ResponseFormat, message: impl Into<String>) -> Self {
        Self {
            format,
            code: 40,
            message: message.into(),
        }
    }

    pub(super) fn not_found(format: ResponseFormat, message: impl Into<String>) -> Self {
        Self {
            format,
            code: 70,
            message: message.into(),
        }
    }
}

impl IntoResponse for SubsonicError {
    fn into_response(self) -> Response {
        subsonic_error_response(self.format, self.code, &self.message)
    }
}

pub(super) fn map_app_error(format: ResponseFormat, error: AppError) -> SubsonicError {
    match error {
        AppError::NoActiveSession => SubsonicError::generic(format, "no active session"),
        AppError::UpstreamHttp { message, .. }
        | AppError::Command(message)
        | AppError::Message(message)
        | AppError::Protocol(message)
        | AppError::Native(message)
        | AppError::InvalidDeviceInfo(message) => SubsonicError::generic(format, message),
        other => SubsonicError::generic(format, other.to_string()),
    }
}
