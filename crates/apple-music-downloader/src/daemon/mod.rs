mod subsonic;

use std::sync::Arc;

use apple_music_api::{
    AppleApiClient, AppleMusicApiError, ArtistViewRequest, Artwork, SearchRequest,
    SongPlaybackMetadata,
};
use apple_music_decryptor::{
    AppleMusicDecryptorError, ArtworkDescriptor, BinaryHealth, LoginAttempt, LoginWaitState,
    NativePlatform, PlaybackOutput, PlaybackRequest, PlaybackTrackMetadata, SessionRuntime,
    ToolHealthReport, download_playback, tool_health_report,
};
use axum::extract::{Path, Query, Request, State};
use axum::http::StatusCode;
use axum::http::header::{AUTHORIZATION, HeaderValue, RETRY_AFTER, WWW_AUTHENTICATE};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tower_http::services::ServeDir;

use crate::config::AppConfig;
use crate::error::{AppError, AppResult};
use crate::runtime::AppState;

#[derive(Clone)]
pub(crate) struct DaemonContext {
    config: AppConfig,
    platform: Arc<NativePlatform>,
    state: Arc<AppState>,
    api: AppleApiClient,
}

impl DaemonContext {
    pub(crate) fn new(
        config: AppConfig,
        platform: Arc<NativePlatform>,
        state: Arc<AppState>,
    ) -> AppResult<Self> {
        Ok(Self {
            api: AppleApiClient::new(config.proxy.as_deref())?,
            config,
            platform,
            state,
        })
    }

    pub(crate) fn session(&self) -> AppResult<Arc<SessionRuntime>> {
        self.state.session().ok_or(AppError::NoActiveSession)
    }

    pub(crate) fn default_storefront(&self) -> &str {
        &self.config.storefront
    }

    pub(crate) fn default_language(&self) -> Option<&str> {
        (!self.config.language.is_empty()).then_some(self.config.language.as_str())
    }

    pub(crate) fn api_token(&self) -> &str {
        &self.config.api_token
    }

    pub(crate) fn subsonic_username(&self) -> &str {
        &self.config.subsonic_username
    }

    pub(crate) fn subsonic_password(&self) -> &str {
        &self.config.subsonic_password
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
    let legacy_routes = Router::new()
        .route("/health", get(health_handler))
        .route("/status", get(status_handler))
        .route("/login", post(login_handler))
        .route("/login/2fa", post(submit_two_factor_handler))
        .route("/login/reset", post(reset_login_handler))
        .route("/logout", post(logout_handler))
        .route("/search", get(search_handler))
        .route("/artist/{id}", get(artist_handler))
        .route("/artist/{id}/view/{name}", get(artist_view_handler))
        .route("/album/{id}", get(album_handler))
        .route("/song/{id}", get(song_handler))
        .route("/lyrics/{id}", get(lyrics_handler))
        .route("/playback/{id}", get(playback_handler))
        .nest_service("/cache", ServeDir::new(config.cache_dir.clone()))
        .layer(middleware::from_fn_with_state(
            Arc::clone(&context),
            require_bearer_auth,
        ));
    let app = Router::new()
        .merge(legacy_routes)
        .merge(subsonic::router(Arc::clone(&context)))
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
struct ArtistParams {
    storefront: Option<String>,
    views: Option<String>,
    limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct ArtistViewParams {
    storefront: Option<String>,
    limit: Option<usize>,
    offset: Option<usize>,
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

#[derive(Debug, Serialize)]
struct ErrorResponse {
    status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    state: Option<&'static str>,
    message: String,
}

#[derive(Debug, Serialize)]
struct HealthResponse {
    status: &'static str,
    state: &'static str,
    version: &'static str,
    ffmpeg: BinaryHealth,
    ffprobe: BinaryHealth,
}

fn default_search_limit() -> usize {
    10
}

fn default_search_type() -> String {
    "song".into()
}

fn resolve_storefront(
    requested: Option<&str>,
    session: Option<&SessionRuntime>,
    configured: &str,
) -> String {
    if let Some(storefront) = requested
        .map(str::trim)
        .filter(|value| value.len() == 2)
        .map(|value| value.to_ascii_lowercase())
    {
        return storefront;
    }

    // Apple serves playback and lyrics against the account storefront even when search works in
    // other catalogs. Using the session storefront keeps the whole download chain consistent.
    if let Some(storefront) = session
        .map(|session| {
            session
                .account_profile()
                .storefront_id
                .trim()
                .to_ascii_lowercase()
        })
        .filter(|value| value.len() == 2)
    {
        return storefront;
    }

    configured.to_owned()
}

async fn status_handler(
    State(context): State<Arc<DaemonContext>>,
) -> Result<Json<AuthResponse>, ApiError> {
    Ok(Json(AuthResponse::ok(state_name(&context.state))))
}

async fn health_handler(State(context): State<Arc<DaemonContext>>) -> Response {
    let (status, body) = health_response(&context.state, tool_health_report());
    (status, Json(body)).into_response()
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
        LoginWaitState::Completed(result) => match *result {
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

    let storefront = resolve_storefront(
        params.storefront.as_deref(),
        context.state.session().as_deref(),
        context.default_storefront(),
    );
    let response = context
        .api
        .search(SearchRequest {
            storefront: &storefront,
            language: context.default_language(),
            query: &params.query,
            search_type: &params.search_type,
            limit: params.limit,
            offset: params.offset,
        })
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
    let storefront = resolve_storefront(
        params.storefront.as_deref(),
        Some(session.as_ref()),
        context.default_storefront(),
    );
    let response = context
        .api
        .album(
            &storefront,
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
    let storefront = resolve_storefront(
        params.storefront.as_deref(),
        Some(session.as_ref()),
        context.default_storefront(),
    );
    let response = context
        .api
        .song(
            &storefront,
            context.default_language(),
            &profile.dev_token,
            &song_id,
        )
        .await?;
    Ok(Json(response))
}

async fn artist_handler(
    State(context): State<Arc<DaemonContext>>,
    Path(artist_id): Path<String>,
    Query(params): Query<ArtistParams>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let session = context.session()?;
    let profile = session.account_profile();
    let storefront = resolve_storefront(
        params.storefront.as_deref(),
        Some(session.as_ref()),
        context.default_storefront(),
    );
    let response = context
        .api
        .artist(
            &storefront,
            context.default_language(),
            &profile.dev_token,
            &artist_id,
            params.views.as_deref(),
            params.limit,
        )
        .await?;
    Ok(Json(response))
}

async fn artist_view_handler(
    State(context): State<Arc<DaemonContext>>,
    Path((artist_id, view_name)): Path<(String, String)>,
    Query(params): Query<ArtistViewParams>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let session = context.session()?;
    let profile = session.account_profile();
    let storefront = resolve_storefront(
        params.storefront.as_deref(),
        Some(session.as_ref()),
        context.default_storefront(),
    );
    let response = context
        .api
        .artist_view(ArtistViewRequest {
            storefront: &storefront,
            language: context.default_language(),
            dev_token: &profile.dev_token,
            artist_id: &artist_id,
            view_name: &view_name,
            limit: params.limit,
            offset: params.offset,
        })
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
    let lyrics_music_token = context
        .config
        .media_user_token
        .as_deref()
        .unwrap_or(&profile.music_token);
    let storefront = resolve_storefront(
        params.storefront.as_deref(),
        Some(session.as_ref()),
        context.default_storefront(),
    );
    let lyrics = context
        .api
        .lyrics(
            &storefront,
            context.default_language(),
            lyrics_music_token,
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
    let profile = session.account_profile();
    let lyrics_music_token = context
        .config
        .media_user_token
        .as_deref()
        .unwrap_or(&profile.music_token);
    let storefront = resolve_storefront(
        params.storefront.as_deref(),
        Some(session.as_ref()),
        context.default_storefront(),
    );
    let metadata = context
        .api
        .song_playback_metadata(
            &storefront,
            context.default_language(),
            &profile.dev_token,
            &song_id,
        )
        .await?;
    // Keep audio downloads working for tracks that Apple serves without lyrics.
    let lyrics = match context
        .api
        .lyrics(
            &storefront,
            context.default_language(),
            lyrics_music_token,
            &song_id,
        )
        .await
    {
        Ok(lyrics) => Some(lyrics),
        Err(error) => {
            crate::app_warn!(
                "daemon::playback",
                "lyrics fetch failed for song {}: {}",
                song_id,
                error
            );
            None
        }
    };
    let config = context.config.download_config();
    let request = PlaybackRequest {
        metadata: playback_track_metadata(metadata, lyrics),
        requested_codec: params.codec.clone(),
    };

    let playback = tokio::task::spawn_blocking(move || download_playback(config, session, request))
        .await
        .map_err(|error| ApiError::internal(format!("playback task panicked: {error}")))??;

    if params.redirect {
        return Ok(Redirect::temporary(&format!("/{}", playback.relative_path)).into_response());
    }

    Ok(Json(playback_response(playback)).into_response())
}

async fn require_bearer_auth(
    State(context): State<Arc<DaemonContext>>,
    request: Request,
    next: Next,
) -> Result<Response, ApiError> {
    let path = request.uri().path().to_owned();
    let Some(header) = request.headers().get(AUTHORIZATION) else {
        return Err(ApiError::unauthorized(&path, "missing bearer token"));
    };
    let Ok(header) = header.to_str() else {
        return Err(ApiError::unauthorized(
            &path,
            "invalid authorization header encoding",
        ));
    };
    let Some(token) = header.strip_prefix("Bearer ") else {
        return Err(ApiError::unauthorized(
            &path,
            "authorization header must use Bearer",
        ));
    };
    if token != context.api_token() {
        return Err(ApiError::unauthorized(&path, "invalid bearer token"));
    }
    Ok(next.run(request).await)
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

fn playback_track_metadata(
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

struct ApiError {
    status: StatusCode,
    state: Option<&'static str>,
    message: String,
    retry_after: Option<String>,
    www_authenticate: Option<&'static str>,
}

impl ApiError {
    fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            state: None,
            message: message.into(),
            retry_after: None,
            www_authenticate: None,
        }
    }

    fn conflict(state: &'static str, message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::CONFLICT,
            state: Some(state),
            message: message.into(),
            retry_after: None,
            www_authenticate: None,
        }
    }

    fn internal(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            state: None,
            message: message.into(),
            retry_after: None,
            www_authenticate: None,
        }
    }

    fn unauthorized(path: &str, message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            state: None,
            message: format!("unauthorized for {path}: {}", message.into()),
            retry_after: None,
            www_authenticate: Some("Bearer"),
        }
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

fn state_name(state: &AppState) -> &'static str {
    if state.pending_login().is_some() {
        "awaiting_2fa"
    } else if state.session().is_some() {
        "logged_in"
    } else {
        "logged_out"
    }
}

fn health_response(state: &AppState, report: ToolHealthReport) -> (StatusCode, HealthResponse) {
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
    use std::process::Command;

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
        let output = Command::new("git")
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
