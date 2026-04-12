use apple_music_api::{AppleMusicApiError, Artwork, SongPlaybackMetadata};
use apple_music_decryptor::{
    AppleMusicDecryptorError, ArtworkDescriptor, BinaryHealth, PlaybackOutput,
    PlaybackTrackMetadata, ToolHealthReport,
};
use axum::Json;
use axum::http::StatusCode;
use axum::http::header::{HeaderValue, RETRY_AFTER, WWW_AUTHENTICATE};
use axum::response::{IntoResponse, Response};
use serde::Serialize;
use serde_json::json;

use crate::error::AppError;
use crate::runtime::AppState;

#[derive(Debug, Serialize)]
struct ErrorResponse {
    status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    state: Option<&'static str>,
    message: String,
}

#[derive(Debug, Serialize)]
pub(super) struct HealthResponse {
    status: &'static str,
    state: &'static str,
    version: &'static str,
    ffmpeg: BinaryHealth,
    ffprobe: BinaryHealth,
}

pub(super) fn playback_response(playback: PlaybackOutput) -> serde_json::Value {
    json!({
        "playbackUrl": playback.relative_path,
        "size": playback.size,
        "artist": playback.artist,
        "artistId": playback.artist_id,
        "albumId": playback.album_id,
        "album": playback.album,
        "title": playback.title,
        "codec": playback.codec,
    })
}

pub(super) fn playback_track_metadata(
    metadata: SongPlaybackMetadata,
    lyrics: Option<String>,
) -> PlaybackTrackMetadata {
    PlaybackTrackMetadata {
        song_id: metadata.song_id,
        artist: metadata.artist,
        artist_id: metadata.artist_id,
        album_id: metadata.album_id,
        album: metadata.album,
        title: metadata.title,
        track_number: metadata.track_number,
        disc_number: metadata.disc_number,
        artwork: metadata.artwork.map(playback_artwork),
        album_artwork: metadata.album_artwork.map(playback_artwork),
        lyrics,
    }
}

fn playback_artwork(artwork: Artwork) -> ArtworkDescriptor {
    ArtworkDescriptor {
        url: artwork.url,
        width: artwork.width,
        height: artwork.height,
    }
}

pub(super) struct ApiError {
    status: StatusCode,
    state: Option<&'static str>,
    message: String,
    retry_after: Option<String>,
    www_authenticate: Option<&'static str>,
}

impl ApiError {
    pub(super) fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            state: None,
            message: message.into(),
            retry_after: None,
            www_authenticate: None,
        }
    }

    pub(super) fn conflict(state: &'static str, message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::CONFLICT,
            state: Some(state),
            message: message.into(),
            retry_after: None,
            www_authenticate: None,
        }
    }

    pub(super) fn internal(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            state: None,
            message: message.into(),
            retry_after: None,
            www_authenticate: None,
        }
    }

    pub(super) fn unauthorized(path: &str, message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            state: None,
            message: format!("unauthorized for {path}: {}", message.into()),
            retry_after: None,
            www_authenticate: Some("Bearer"),
        }
    }

    pub(super) fn message(&self) -> &str {
        &self.message
    }
}

impl From<AppError> for ApiError {
    fn from(error: AppError) -> Self {
        match error {
            AppError::NoActiveSession => Self {
                status: StatusCode::CONFLICT,
                state: Some("logged_out"),
                message: "no active session".into(),
                retry_after: None,
                www_authenticate: None,
            },
            AppError::Protocol(message)
            | AppError::InvalidDeviceInfo(message)
            | AppError::Native(message)
            | AppError::Message(message) => Self {
                status: StatusCode::BAD_REQUEST,
                state: None,
                message,
                retry_after: None,
                www_authenticate: None,
            },
            AppError::UpstreamHttp {
                status,
                message,
                retry_after,
            } => Self {
                status: StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::BAD_GATEWAY),
                state: None,
                message,
                retry_after,
                www_authenticate: None,
            },
            AppError::Command(message) => Self::internal(message),
            other => Self::internal(other.to_string()),
        }
    }
}

impl From<std::io::Error> for ApiError {
    fn from(error: std::io::Error) -> Self {
        Self::internal(error.to_string())
    }
}

impl From<AppleMusicApiError> for ApiError {
    fn from(error: AppleMusicApiError) -> Self {
        AppError::from(error).into()
    }
}

impl From<AppleMusicDecryptorError> for ApiError {
    fn from(error: AppleMusicDecryptorError) -> Self {
        AppError::from(error).into()
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let mut response = (
            self.status,
            Json(ErrorResponse {
                status: "error",
                state: self.state,
                message: self.message,
            }),
        )
            .into_response();
        if let Some(retry_after) = self.retry_after.as_deref()
            && let Ok(value) = HeaderValue::from_str(retry_after)
        {
            response.headers_mut().insert(RETRY_AFTER, value);
        }
        if let Some(www_authenticate) = self.www_authenticate
            && let Ok(value) = HeaderValue::from_str(www_authenticate)
        {
            response.headers_mut().insert(WWW_AUTHENTICATE, value);
        }
        response
    }
}

pub(super) fn state_name(state: &AppState) -> &'static str {
    if state.pending_login().is_some() {
        "awaiting_2fa"
    } else if state.session().is_some() {
        "logged_in"
    } else {
        "logged_out"
    }
}

pub(super) fn health_response(
    state: &AppState,
    report: ToolHealthReport,
) -> (StatusCode, HealthResponse) {
    let healthy = report.is_healthy();
    let status = if healthy {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    (
        status,
        HealthResponse {
            status: if healthy { "ok" } else { "degraded" },
            state: state_name(state),
            version: crate::BUILD_VERSION,
            ffmpeg: report.ffmpeg,
            ffprobe: report.ffprobe,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::{ApiError, ErrorResponse, health_response};

    use apple_music_decryptor::{BinaryHealth, ToolHealthReport};
    use axum::http::StatusCode;
    use axum::response::IntoResponse;

    use crate::error::AppError;
    use crate::runtime::AppState;

    #[test]
    fn conflict_errors_serialize_request_status_separately_from_session_state() {
        let response = ErrorResponse {
            status: "error",
            state: Some("logged_out"),
            message: "no active session".into(),
        };
        let json = serde_json::to_string(&response).expect("serialize error response");
        assert!(json.contains("\"status\":\"error\""));
        assert!(json.contains("\"state\":\"logged_out\""));
        assert!(json.contains("\"message\":\"no active session\""));
    }

    #[test]
    fn bad_request_errors_do_not_invent_logged_out_state() {
        let error = ApiError::bad_request("query parameter is required");
        assert_eq!(error.status, StatusCode::BAD_REQUEST);
        assert_eq!(error.state, None);
        assert_eq!(error.message, "query parameter is required");
    }

    #[test]
    fn upstream_429_preserves_status_and_retry_after_header() {
        let response = ApiError::from(AppError::UpstreamHttp {
            status: reqwest::StatusCode::TOO_MANY_REQUESTS,
            message: "apple api request failed: 429 Too Many Requests".into(),
            retry_after: Some("3".into()),
        })
        .into_response();
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(
            response
                .headers()
                .get(axum::http::header::RETRY_AFTER)
                .expect("retry-after header"),
            "3"
        );
    }

    #[test]
    fn unauthorized_errors_set_www_authenticate_header() {
        let response = ApiError::unauthorized("/search", "missing bearer token").into_response();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            response
                .headers()
                .get(axum::http::header::WWW_AUTHENTICATE)
                .expect("www-authenticate header"),
            "Bearer"
        );
    }

    #[test]
    fn health_response_includes_build_version() {
        let (status, body) = health_response(
            &AppState::default(),
            ToolHealthReport {
                ffmpeg: available_binary("/usr/local/bin/ffmpeg"),
                ffprobe: available_binary("/usr/local/bin/ffprobe"),
            },
        );
        assert_eq!(status, StatusCode::OK);

        let json = serde_json::to_value(&body).expect("serialize health response");
        assert_eq!(json["status"], "ok");
        assert_eq!(json["state"], "logged_out");
        assert_eq!(json["version"], crate::BUILD_VERSION);
    }

    #[test]
    fn build_version_uses_short_lowercase_git_prefix() {
        assert_eq!(crate::BUILD_VERSION.len(), 8);
        assert!(
            crate::BUILD_VERSION
                .chars()
                .all(|ch| matches!(ch, '0'..='9' | 'a'..='f'))
        );
    }

    #[test]
    fn build_version_matches_git_head_prefix() {
        let output = std::process::Command::new("git")
            .args(["rev-parse", "--verify", "HEAD"])
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .output()
            .expect("read git head");
        assert!(output.status.success(), "git rev-parse HEAD failed");

        let head = String::from_utf8(output.stdout).expect("git head is utf-8");
        assert_eq!(crate::BUILD_VERSION, &head.trim()[..8]);
    }

    fn available_binary(path: &'static str) -> BinaryHealth {
        BinaryHealth {
            path,
            available: true,
            version: Some("test".into()),
            error: None,
        }
    }
}
