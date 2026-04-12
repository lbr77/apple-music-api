use std::sync::Arc;

use apple_music_api::{ArtistViewRequest, SearchRequest};
use apple_music_decryptor::download_playback;
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::response::{IntoResponse, Redirect, Response};
use serde::Deserialize;
use serde_json::json;

use crate::daemon::context::{DaemonContext, resolve_language, resolve_storefront};
use crate::daemon::response::{ApiError, playback_response, playback_track_metadata};

#[derive(Debug, Deserialize)]
pub(super) struct SearchParams {
    query: String,
    #[serde(default = "default_search_limit")]
    limit: usize,
    #[serde(default)]
    offset: usize,
    #[serde(rename = "type", default = "default_search_type")]
    search_type: String,
    storefront: Option<String>,
    language: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct StorefrontParams {
    storefront: Option<String>,
    language: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct ArtistParams {
    storefront: Option<String>,
    language: Option<String>,
    views: Option<String>,
    limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub(super) struct ArtistViewParams {
    storefront: Option<String>,
    language: Option<String>,
    limit: Option<usize>,
    offset: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub(super) struct PlaybackParams {
    storefront: Option<String>,
    language: Option<String>,
    #[serde(default)]
    redirect: bool,
    codec: Option<String>,
}

fn default_search_limit() -> usize {
    10
}

fn default_search_type() -> String {
    "song".into()
}

pub(super) async fn search_handler(
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
    let language = resolve_language(params.language.as_deref(), context.default_language());
    crate::app_debug!(
        "http::catalog",
        "search requested: type={}, query_len={}, limit={}, offset={}, storefront={}, language={}",
        params.search_type,
        params.query.trim().len(),
        params.limit,
        params.offset,
        storefront,
        language.unwrap_or_default(),
    );
    let response = context
        .api
        .search(SearchRequest {
            storefront: &storefront,
            language,
            query: &params.query,
            search_type: &params.search_type,
            limit: params.limit,
            offset: params.offset,
        })
        .await?;
    crate::app_debug!(
        "http::catalog",
        "search finished: type={}",
        params.search_type
    );
    Ok(Json(response))
}

pub(super) async fn album_handler(
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
    let language = resolve_language(params.language.as_deref(), context.default_language());
    crate::app_debug!(
        "http::catalog",
        "album requested: album_id={}, storefront={}, language={}",
        album_id,
        storefront,
        language.unwrap_or_default(),
    );
    let response = context
        .api
        .album(&storefront, language, &profile.dev_token, &album_id)
        .await?;
    Ok(Json(response))
}

pub(super) async fn song_handler(
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
    let language = resolve_language(params.language.as_deref(), context.default_language());
    crate::app_debug!(
        "http::catalog",
        "song requested: song_id={}, storefront={}, language={}",
        song_id,
        storefront,
        language.unwrap_or_default(),
    );
    let response = context
        .api
        .song(&storefront, language, &profile.dev_token, &song_id)
        .await?;
    Ok(Json(response))
}

pub(super) async fn artist_handler(
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
    let language = resolve_language(params.language.as_deref(), context.default_language());
    crate::app_debug!(
        "http::catalog",
        "artist requested: artist_id={}, storefront={}, language={}, views={}, limit={}",
        artist_id,
        storefront,
        language.unwrap_or_default(),
        params.views.as_deref().unwrap_or_default(),
        params.limit.unwrap_or_default(),
    );
    let response = context
        .api
        .artist(
            &storefront,
            language,
            &profile.dev_token,
            &artist_id,
            params.views.as_deref(),
            params.limit,
        )
        .await?;
    Ok(Json(response))
}

pub(super) async fn artist_view_handler(
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
    let language = resolve_language(params.language.as_deref(), context.default_language());
    crate::app_debug!(
        "http::catalog",
        "artist view requested: artist_id={}, view={}, storefront={}, language={}, limit={}, offset={}",
        artist_id,
        view_name,
        storefront,
        language.unwrap_or_default(),
        params.limit.unwrap_or_default(),
        params.offset.unwrap_or_default(),
    );
    let response = context
        .api
        .artist_view(ArtistViewRequest {
            storefront: &storefront,
            language,
            dev_token: &profile.dev_token,
            artist_id: &artist_id,
            view_name: &view_name,
            limit: params.limit,
            offset: params.offset,
        })
        .await?;
    Ok(Json(response))
}

pub(super) async fn lyrics_handler(
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
        crate::app_debug!("http::catalog", "lyrics cache hit: song_id={song_id}");
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
    let language = resolve_language(params.language.as_deref(), context.default_language());
    crate::app_debug!(
        "http::catalog",
        "lyrics requested: song_id={}, storefront={}, language={}, token_source={}",
        song_id,
        storefront,
        language.unwrap_or_default(),
        if context.config.media_user_token.is_some() {
            "config"
        } else {
            "session"
        },
    );
    let lyrics = context
        .api
        .lyrics(&storefront, language, lyrics_music_token, &song_id)
        .await?;

    tokio::fs::write(&cache_path, lyrics.as_bytes()).await?;
    crate::app_debug!("http::catalog", "lyrics cached: song_id={song_id}");
    Ok(Json(json!({ "lyrics": lyrics })))
}

pub(super) async fn playback_handler(
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
    let language = resolve_language(params.language.as_deref(), context.default_language());
    crate::app_debug!(
        "http::playback",
        "playback requested: song_id={}, storefront={}, language={}, redirect={}, codec={}",
        song_id,
        storefront,
        language.unwrap_or_default(),
        params.redirect,
        params.codec.as_deref().unwrap_or_default(),
    );
    let metadata = context
        .api
        .song_playback_metadata(&storefront, language, &profile.dev_token, &song_id)
        .await?;
    let lyrics = match context
        .api
        .lyrics(&storefront, language, lyrics_music_token, &song_id)
        .await
    {
        Ok(lyrics) => Some(lyrics),
        Err(error) => {
            crate::app_warn!(
                "http::playback",
                "lyrics fetch failed for song {}: {}",
                song_id,
                error
            );
            None
        }
    };
    let config = context.config.download_config();
    let request = apple_music_decryptor::PlaybackRequest {
        metadata: playback_track_metadata(metadata, lyrics),
        requested_codec: params.codec.clone(),
    };

    let playback = tokio::task::spawn_blocking(move || download_playback(config, session, request))
        .await
        .map_err(|error| ApiError::internal(format!("playback task panicked: {error}")))??;

    crate::app_debug!(
        "http::playback",
        "playback finished: song_id={}, codec={}, size={}, redirect={}",
        song_id,
        playback.codec,
        playback.size,
        params.redirect,
    );
    if params.redirect {
        return Ok(Redirect::temporary(&format!("/{}", playback.relative_path)).into_response());
    }

    Ok(Json(playback_response(playback)).into_response())
}
