use std::sync::Arc;

use apple_music_decryptor::{LoginAttempt, LoginWaitState, tool_health_report};
use axum::Json;
use axum::extract::State;
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};

use super::log_http_completion;
use crate::daemon::context::DaemonContext;
use crate::daemon::response::{ApiError, health_response, state_name};

#[derive(Debug, Deserialize)]
pub(super) struct LoginPayload {
    username: String,
    password: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct TwoFactorPayload {
    code: String,
}

#[derive(Debug, Serialize)]
pub(super) struct AuthResponse {
    status: &'static str,
    state: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
}

impl AuthResponse {
    fn ok(state: &'static str) -> Self {
        Self {
            status: "ok",
            state,
            message: None,
        }
    }

    fn need_two_factor(message: impl Into<String>) -> Self {
        Self {
            status: "need_2fa",
            state: "awaiting_2fa",
            message: Some(message.into()),
        }
    }
}

pub(super) async fn status_handler(
    State(context): State<Arc<DaemonContext>>,
) -> Result<Json<AuthResponse>, ApiError> {
    let state = state_name(&context.state);
    crate::app_debug!("http::account", "status requested: state={state}");
    Ok(Json(AuthResponse::ok(state)))
}

pub(super) async fn health_handler(State(context): State<Arc<DaemonContext>>) -> Response {
    crate::app_debug!("http::health", "health check requested");
    let session_state = state_name(&context.state);
    let (status, body) = health_response(&context.state, tool_health_report());
    crate::app_debug!(
        "http::health",
        "health check finished: status={}, session_state={}",
        status.as_u16(),
        session_state,
    );
    (status, Json(body)).into_response()
}

pub(super) async fn login_handler(
    State(context): State<Arc<DaemonContext>>,
    Json(payload): Json<LoginPayload>,
) -> Result<Json<AuthResponse>, ApiError> {
    crate::app_debug!(
        "http::account",
        "login requested: username_len={}, has_session={}, awaiting_2fa={}",
        payload.username.len(),
        context.state.session().is_some(),
        context.state.pending_login().is_some(),
    );
    if context.state.session().is_some() {
        return Err(ApiError::conflict(
            "logged_in",
            "logout before starting a new login",
        ));
    }
    if context.state.pending_login().is_some() {
        return Err(ApiError::conflict(
            "awaiting_2fa",
            "a previous login is still waiting for 2FA",
        ));
    }

    let attempt = LoginAttempt::new(payload.username, payload.password);
    let worker_attempt = Arc::clone(&attempt);
    let wait_attempt = Arc::clone(&attempt);
    let platform = Arc::clone(&context.platform);

    tokio::task::spawn_blocking(move || {
        let result = platform.login(Arc::clone(&worker_attempt));
        worker_attempt.finish(result);
    });

    let response = match tokio::task::spawn_blocking(move || wait_attempt.wait_for_initial_state())
        .await
        .map_err(|error| ApiError::internal(format!("login state wait panicked: {error}")))?
    {
        LoginWaitState::NeedTwoFactor => {
            context.state.set_pending_login(attempt);
            Ok(Json(AuthResponse::need_two_factor(
                "verification code required",
            )))
        }
        LoginWaitState::Completed(result) => match *result {
            Ok(session) => {
                let session = apple_music_decryptor::SessionRuntime::new(session)?;
                context.state.replace_session(Arc::new(session));
                Ok(Json(AuthResponse::ok("logged_in")))
            }
            Err(error) => Err(ApiError::conflict("logged_out", error.to_string())),
        },
    };

    match &response {
        Ok(body) => log_http_completion("http::account", "login", Ok(body.state)),
        Err(error) => log_http_completion("http::account", "login", Err(error.message())),
    }
    response
}

pub(super) async fn submit_two_factor_handler(
    State(context): State<Arc<DaemonContext>>,
    Json(payload): Json<TwoFactorPayload>,
) -> Result<Json<AuthResponse>, ApiError> {
    crate::app_debug!(
        "http::account",
        "2FA submission requested: code_len={}",
        payload.code.len(),
    );
    let Some(attempt) = context.state.take_pending_login() else {
        return Err(ApiError::conflict(
            "logged_out",
            "submit_2fa is only valid after login returns need_2fa",
        ));
    };

    attempt.submit_two_factor(payload.code)?;
    let wait_attempt = Arc::clone(&attempt);
    let response = match tokio::task::spawn_blocking(move || wait_attempt.wait_for_completion())
        .await
        .map_err(|error| ApiError::internal(format!("2FA completion wait panicked: {error}")))?
    {
        Ok(session) => {
            let session = apple_music_decryptor::SessionRuntime::new(session)?;
            context.state.replace_session(Arc::new(session));
            Ok(Json(AuthResponse::ok("logged_in")))
        }
        Err(error) => Err(ApiError::conflict("logged_out", error.to_string())),
    };

    match &response {
        Ok(body) => log_http_completion("http::account", "submit_2fa", Ok(body.state)),
        Err(error) => log_http_completion("http::account", "submit_2fa", Err(error.message())),
    }
    response
}

pub(super) async fn reset_login_handler(
    State(context): State<Arc<DaemonContext>>,
) -> Result<Json<AuthResponse>, ApiError> {
    crate::app_debug!(
        "http::account",
        "login reset requested: has_session={}, awaiting_2fa={}",
        context.state.session().is_some(),
        context.state.pending_login().is_some(),
    );
    if context.state.session().is_some() {
        return Err(ApiError::conflict(
            "logged_in",
            "use logout to clear the active session",
        ));
    }
    if let Some(attempt) = context.state.clear_pending_login() {
        attempt.cancel("login reset by daemon command");
    }
    log_http_completion("http::account", "login_reset", Ok("logged_out"));
    Ok(Json(AuthResponse::ok("logged_out")))
}

pub(super) async fn logout_handler(
    State(context): State<Arc<DaemonContext>>,
) -> Result<Json<AuthResponse>, ApiError> {
    crate::app_debug!(
        "http::account",
        "logout requested: has_session={}, awaiting_2fa={}",
        context.state.session().is_some(),
        context.state.pending_login().is_some(),
    );
    if context.state.pending_login().is_some() {
        return Err(ApiError::conflict(
            "awaiting_2fa",
            "cannot logout while a login is waiting for 2FA",
        ));
    }
    if let Some(session) = context.state.clear_session() {
        tokio::task::spawn_blocking(move || session.native().logout())
            .await
            .map_err(|error| ApiError::internal(format!("logout task panicked: {error}")))??;
    }
    log_http_completion("http::account", "logout", Ok("logged_out"));
    Ok(Json(AuthResponse::ok("logged_out")))
}

#[cfg(test)]
mod tests {
    use super::health_response;
    use apple_music_decryptor::{BinaryHealth, ToolHealthReport};
    use axum::http::StatusCode;

    use crate::runtime::AppState;

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
