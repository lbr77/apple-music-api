mod api;
mod download;
pub(crate) mod mp4;

use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tower_http::services::ServeDir;

use crate::config::AppConfig;
use crate::error::{AppError, AppResult};
use crate::ffi::{LoginAttempt, LoginWaitState, NativePlatform};
use crate::runtime::{AppState, SessionRuntime};

use self::api::AppleApiClient;
use self::download::{PlaybackOutput, tool_health_report};

#[derive(Clone)]
struct DaemonContext {
    config: AppConfig,
    platform: Arc<NativePlatform>,
    state: Arc<AppState>,
    api: AppleApiClient,
}

impl DaemonContext {
    fn new(
        config: AppConfig,
        platform: Arc<NativePlatform>,
        state: Arc<AppState>,
    ) -> AppResult<Self> {
        Ok(Self {
            api: AppleApiClient::new(&config)?,
            config,
            platform,
            state,
        })
    }

    fn session(&self) -> AppResult<Arc<SessionRuntime>> {
        self.state.session().ok_or(AppError::NoActiveSession)
    }

    fn default_storefront(&self) -> &str {
        &self.config.storefront
    }

    fn default_language(&self) -> Option<&str> {
        (!self.config.language.is_empty()).then_some(self.config.language.as_str())
    }
}

pub async fn run_daemon_server(
    config: AppConfig,
    platform: Arc<NativePlatform>,
    state: Arc<AppState>,
) -> AppResult<()> {
    std::fs::create_dir_all(config.cache_dir.join("lyrics"))?;
    std::fs::create_dir_all(config.cache_dir.join("albums"))?;

    let context = Arc::new(DaemonContext::new(config.clone(), platform, state)?);
    let app = Router::new()
        .route("/health", get(health_handler))
        .route("/status", get(status_handler))
        .route("/login", post(login_handler))
        .route("/login/2fa", post(submit_two_factor_handler))
        .route("/login/reset", post(reset_login_handler))
        .route("/logout", post(logout_handler))
        .route("/search", get(search_handler))
        .route("/album/{id}", get(album_handler))
        .route("/song/{id}", get(song_handler))
        .route("/lyrics/{id}", get(lyrics_handler))
        .route("/playback/{id}", get(playback_handler))
        .nest_service("/cache", ServeDir::new(config.cache_dir.clone()))
        .with_state(context);

    let listener = tokio::net::TcpListener::bind(config.daemon_addr()).await?;
    crate::app_info!(
        "daemon",
        "listening for daemon http requests on {}",
        config.daemon_addr(),
    );
    axum::serve(listener, app)
        .await
        .map_err(|error| AppError::Message(format!("daemon server failed: {error}")))?;
    Ok(())
}

#[derive(Debug, Deserialize)]
struct LoginPayload {
    username: String,
    password: String,
}

#[derive(Debug, Deserialize)]
struct TwoFactorPayload {
    code: String,
}

#[derive(Debug, Deserialize)]
struct SearchParams {
    query: String,
    #[serde(default = "default_search_limit")]
    limit: usize,
    #[serde(default)]
    offset: usize,
    #[serde(rename = "type", default = "default_search_type")]
    search_type: String,
    storefront: Option<String>,
}

#[derive(Debug, Deserialize)]
struct StorefrontParams {
    storefront: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PlaybackParams {
    storefront: Option<String>,
    #[serde(default)]
    redirect: bool,
    codec: Option<String>,
}

#[derive(Debug, Serialize)]
struct AuthResponse {
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

fn default_search_limit() -> usize {
    10
}

fn default_search_type() -> String {
    "song".into()
}

async fn status_handler(
    State(context): State<Arc<DaemonContext>>,
) -> Result<Json<AuthResponse>, ApiError> {
    Ok(Json(AuthResponse::ok(state_name(&context.state))))
}

async fn health_handler(State(context): State<Arc<DaemonContext>>) -> Response {
    let report = tool_health_report();
    let healthy = report.is_healthy();
    let status = if healthy {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    (
        status,
        Json(json!({
            "status": if healthy { "ok" } else { "degraded" },
            "state": state_name(&context.state),
            "ffmpeg": report.ffmpeg,
            "ffprobe": report.ffprobe,
        })),
    )
        .into_response()
}

async fn login_handler(
    State(context): State<Arc<DaemonContext>>,
    Json(payload): Json<LoginPayload>,
) -> Result<Json<AuthResponse>, ApiError> {
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

    match tokio::task::spawn_blocking(move || wait_attempt.wait_for_initial_state())
        .await
        .map_err(|error| ApiError::internal(format!("login state wait panicked: {error}")))?
    {
        LoginWaitState::NeedTwoFactor => {
            context.state.set_pending_login(attempt);
            Ok(Json(AuthResponse::need_two_factor(
                "verification code required",
            )))
        }
        LoginWaitState::Completed(result) => match result {
            Ok(session) => {
                let session = SessionRuntime::new(session)?;
                context.state.replace_session(Arc::new(session));
                Ok(Json(AuthResponse::ok("logged_in")))
            }
            Err(error) => Err(ApiError::conflict("logged_out", error.to_string())),
        },
    }
}

async fn submit_two_factor_handler(
    State(context): State<Arc<DaemonContext>>,
    Json(payload): Json<TwoFactorPayload>,
) -> Result<Json<AuthResponse>, ApiError> {
    let Some(attempt) = context.state.take_pending_login() else {
        return Err(ApiError::conflict(
            "logged_out",
            "submit_2fa is only valid after login returns need_2fa",
        ));
    };

    attempt.submit_two_factor(payload.code)?;
    let wait_attempt = Arc::clone(&attempt);
    match tokio::task::spawn_blocking(move || wait_attempt.wait_for_completion())
        .await
        .map_err(|error| ApiError::internal(format!("2FA completion wait panicked: {error}")))?
    {
        Ok(session) => {
            let session = SessionRuntime::new(session)?;
            context.state.replace_session(Arc::new(session));
            Ok(Json(AuthResponse::ok("logged_in")))
        }
        Err(error) => Err(ApiError::conflict("logged_out", error.to_string())),
    }
}

async fn reset_login_handler(
    State(context): State<Arc<DaemonContext>>,
) -> Result<Json<AuthResponse>, ApiError> {
    if context.state.session().is_some() {
        return Err(ApiError::conflict(
            "logged_in",
            "use logout to clear the active session",
        ));
    }
    if let Some(attempt) = context.state.clear_pending_login() {
        attempt.cancel("login reset by daemon command");
    }
    Ok(Json(AuthResponse::ok("logged_out")))
}

async fn logout_handler(
    State(context): State<Arc<DaemonContext>>,
) -> Result<Json<AuthResponse>, ApiError> {
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
    Ok(Json(AuthResponse::ok("logged_out")))
}

async fn search_handler(
    State(context): State<Arc<DaemonContext>>,
    Query(params): Query<SearchParams>,
) -> Result<Json<serde_json::Value>, ApiError> {
    if params.query.trim().is_empty() {
        return Err(ApiError::bad_request("query parameter is required"));
    }
    if !matches!(params.search_type.as_str(), "song" | "album" | "artist") {
        return Err(ApiError::bad_request(format!(
            "invalid search type: {}. Use 'album', 'song', or 'artist'",
            params.search_type
        )));
    }

    let session = context.session()?;
    let profile = session.account_profile();
    let storefront = params
        .storefront
        .as_deref()
        .unwrap_or(context.default_storefront());
    let response = context
        .api
        .search(
            storefront,
            context.default_language(),
            &profile.dev_token,
            &params.query,
            &params.search_type,
            params.limit,
            params.offset,
        )
        .await?;
    Ok(Json(response))
}

async fn album_handler(
    State(context): State<Arc<DaemonContext>>,
    Path(album_id): Path<String>,
    Query(params): Query<StorefrontParams>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let session = context.session()?;
    let profile = session.account_profile();
    let storefront = params
        .storefront
        .as_deref()
        .unwrap_or(context.default_storefront());
    let response = context
        .api
        .album(
            storefront,
            context.default_language(),
            &profile.dev_token,
            &album_id,
        )
        .await?;
    Ok(Json(response))
}

async fn song_handler(
    State(context): State<Arc<DaemonContext>>,
    Path(song_id): Path<String>,
    Query(params): Query<StorefrontParams>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let session = context.session()?;
    let profile = session.account_profile();
    let storefront = params
        .storefront
        .as_deref()
        .unwrap_or(context.default_storefront());
    let response = context
        .api
        .song(
            storefront,
            context.default_language(),
            &profile.dev_token,
            &song_id,
        )
        .await?;
    Ok(Json(response))
}

async fn lyrics_handler(
    State(context): State<Arc<DaemonContext>>,
    Path(song_id): Path<String>,
    Query(params): Query<StorefrontParams>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let cache_path = context
        .config
        .cache_dir
        .join("lyrics")
        .join(format!("{song_id}.lrc"));
    if cache_path.is_file() {
        let lyrics = tokio::fs::read_to_string(&cache_path).await?;
        return Ok(Json(json!({ "lyrics": lyrics })));
    }

    let session = context.session()?;
    let profile = session.account_profile();
    let storefront = params
        .storefront
        .as_deref()
        .unwrap_or(context.default_storefront());
    let lyrics = context
        .api
        .lyrics(
            storefront,
            context.default_language(),
            &profile.dev_token,
            &profile.music_token,
            &song_id,
        )
        .await?;

    tokio::fs::write(&cache_path, lyrics.as_bytes()).await?;
    Ok(Json(json!({ "lyrics": lyrics })))
}

async fn playback_handler(
    State(context): State<Arc<DaemonContext>>,
    Path(song_id): Path<String>,
    Query(params): Query<PlaybackParams>,
) -> Result<Response, ApiError> {
    let session = context.session()?;
    let storefront = params
        .storefront
        .clone()
        .unwrap_or_else(|| context.default_storefront().to_owned());
    let language = context.default_language().map(str::to_owned);
    let config = context.config.clone();
    let codec = params.codec.clone();

    let playback = tokio::task::spawn_blocking(move || {
        download::download_playback(config, session, storefront, language, song_id, codec)
    })
    .await
    .map_err(|error| ApiError::internal(format!("playback task panicked: {error}")))??;

    if params.redirect {
        return Ok(Redirect::temporary(&format!("/{}", playback.relative_path)).into_response());
    }

    Ok(Json(playback_response(playback)).into_response())
}

fn playback_response(playback: PlaybackOutput) -> serde_json::Value {
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

struct ApiError {
    status: StatusCode,
    state: &'static str,
    message: String,
}

impl ApiError {
    fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            state: "logged_out",
            message: message.into(),
        }
    }

    fn conflict(state: &'static str, message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::CONFLICT,
            state,
            message: message.into(),
        }
    }

    fn internal(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            state: "logged_out",
            message: message.into(),
        }
    }
}

impl From<AppError> for ApiError {
    fn from(error: AppError) -> Self {
        match error {
            AppError::NoActiveSession => Self {
                status: StatusCode::CONFLICT,
                state: "logged_out",
                message: "no active session".into(),
            },
            AppError::Protocol(message)
            | AppError::InvalidDeviceInfo(message)
            | AppError::Native(message)
            | AppError::Message(message) => Self {
                status: StatusCode::BAD_REQUEST,
                state: "logged_out",
                message,
            },
            other => Self::internal(other.to_string()),
        }
    }
}

impl From<std::io::Error> for ApiError {
    fn from(error: std::io::Error) -> Self {
        Self::internal(error.to_string())
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(json!({
                "state": self.state,
                "error": self.message,
            })),
        )
            .into_response()
    }
}

fn state_name(state: &AppState) -> &'static str {
    if state.pending_login().is_some() {
        "awaiting_2fa"
    } else if state.session().is_some() {
        "logged_in"
    } else {
        "logged_out"
    }
}
