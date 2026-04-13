mod auth;
mod data;
mod error;
mod handlers;
mod render;
mod service;

use std::sync::Arc;

use axum::Router;
use axum::middleware;
use axum::routing::get;
use serde::Deserialize;

use super::DaemonContext;

const SUBSONIC_API_VERSION: &str = "1.16.1";
const SUBSONIC_SERVER_TYPE: &str = "wrapper-rs";
const SUBSONIC_MUSIC_FOLDER_ID: i32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ResponseFormat {
    Json,
    Xml,
}

impl ResponseFormat {
    pub(super) fn from_query(raw: Option<&str>) -> Result<Self, &'static str> {
        match raw.unwrap_or("xml").trim().to_ascii_lowercase().as_str() {
            "" | "xml" => Ok(Self::Xml),
            "json" => Ok(Self::Json),
            _ => Err("unsupported response format"),
        }
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct AuthQuery {
    u: Option<String>,
    p: Option<String>,
    t: Option<String>,
    s: Option<String>,
    v: Option<String>,
    c: Option<String>,
    f: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct Search3Query {
    #[serde(flatten)]
    auth: AuthQuery,
    query: String,
    #[serde(rename = "songCount")]
    song_count: Option<usize>,
    #[serde(rename = "songOffset")]
    song_offset: Option<usize>,
    #[serde(rename = "albumCount")]
    album_count: Option<usize>,
    #[serde(rename = "albumOffset")]
    album_offset: Option<usize>,
    #[serde(rename = "artistCount")]
    artist_count: Option<usize>,
    #[serde(rename = "artistOffset")]
    artist_offset: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub(super) struct IdQuery {
    #[serde(flatten)]
    auth: AuthQuery,
    id: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct CoverArtQuery {
    #[serde(flatten)]
    auth: AuthQuery,
    id: String,
    size: Option<u32>,
}

#[derive(Debug, Deserialize)]
pub(super) struct StreamQuery {
    #[serde(flatten)]
    auth: AuthQuery,
    id: String,
    #[serde(rename = "maxBitRate")]
    max_bit_rate: Option<u32>,
    format: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct LyricsQuery {
    #[serde(flatten)]
    auth: AuthQuery,
    artist: Option<String>,
    title: Option<String>,
    id: Option<String>,
}

pub(super) fn router(context: Arc<DaemonContext>) -> Router<Arc<DaemonContext>> {
    Router::new()
        .route("/rest/ping.view", get(handlers::ping_handler))
        .route("/rest/ping", get(handlers::ping_handler))
        .route("/rest/getLicense.view", get(handlers::get_license_handler))
        .route("/rest/getLicense", get(handlers::get_license_handler))
        .route(
            "/rest/getMusicFolders.view",
            get(handlers::get_music_folders_handler),
        )
        .route(
            "/rest/getMusicFolders",
            get(handlers::get_music_folders_handler),
        )
        .route("/rest/getArtists.view", get(handlers::get_artists_handler))
        .route("/rest/getArtists", get(handlers::get_artists_handler))
        .route("/rest/getIndexes.view", get(handlers::get_indexes_handler))
        .route("/rest/getIndexes", get(handlers::get_indexes_handler))
        .route("/rest/search3.view", get(handlers::search3_handler))
        .route("/rest/search3", get(handlers::search3_handler))
        .route("/rest/getArtist.view", get(handlers::get_artist_handler))
        .route("/rest/getArtist", get(handlers::get_artist_handler))
        .route("/rest/getAlbum.view", get(handlers::get_album_handler))
        .route("/rest/getAlbum", get(handlers::get_album_handler))
        .route("/rest/getSong.view", get(handlers::get_song_handler))
        .route("/rest/getSong", get(handlers::get_song_handler))
        .route("/rest/getLyrics.view", get(handlers::get_lyrics_handler))
        .route("/rest/getLyrics", get(handlers::get_lyrics_handler))
        .route(
            "/rest/getCoverArt.view",
            get(handlers::get_cover_art_handler),
        )
        .route("/rest/getCoverArt", get(handlers::get_cover_art_handler))
        .route("/rest/stream.view", get(handlers::stream_handler))
        .route("/rest/stream", get(handlers::stream_handler))
        .layer(middleware::from_fn(handlers::log_subsonic_request))
        .layer(middleware::from_fn_with_state(
            context,
            auth::require_subsonic_auth,
        ))
}

#[cfg(test)]
mod tests {
    use axum::http::StatusCode;
    use reqwest::StatusCode as HttpStatusCode;

    use super::auth::{parse_auth_query, validate_auth_credentials};
    use super::render::{escape_xml_attr, subsonic_error_response};
    use super::service::{lyrics_not_found_as_empty, requested_codec};
    use super::{AuthQuery, ResponseFormat};
    use crate::error::AppError;

    #[test]
    fn parse_auth_query_defaults_to_xml() {
        let query =
            parse_auth_query(Some("u=wrapper&v=1.16.1&c=test&p=secret")).expect("parse auth query");
        assert_eq!(query.f, None);
        assert_eq!(
            ResponseFormat::from_query(query.f.as_deref()).expect("format"),
            ResponseFormat::Xml
        );
    }

    #[test]
    fn validate_auth_accepts_plain_and_token_passwords() {
        validate_auth_credentials(
            "wrapper",
            "secret",
            &AuthQuery {
                u: Some("wrapper".into()),
                p: Some("secret".into()),
                t: None,
                s: None,
                v: Some("1.16.1".into()),
                c: Some("client".into()),
                f: Some("json".into()),
            },
        )
        .expect("plain auth");

        validate_auth_credentials(
            "wrapper",
            "secret",
            &AuthQuery {
                u: Some("wrapper".into()),
                p: None,
                t: Some(format!("{:x}", md5::compute("secret123"))),
                s: Some("123".into()),
                v: Some("1.16.1".into()),
                c: Some("client".into()),
                f: Some("json".into()),
            },
        )
        .expect("token auth");
    }

    #[test]
    fn requested_codec_prefers_aac_when_bitrate_is_limited() {
        assert_eq!(
            requested_codec(Some(320), None, ResponseFormat::Json).expect("codec"),
            Some("aac".into())
        );
        assert_eq!(
            requested_codec(None, Some("alac"), ResponseFormat::Json).expect("codec"),
            Some("alac".into())
        );
    }

    #[test]
    fn requested_codec_keeps_original_stream_when_max_bitrate_is_zero() {
        assert_eq!(
            requested_codec(Some(0), None, ResponseFormat::Json).expect("codec"),
            None
        );
    }

    #[test]
    fn lyrics_not_found_maps_to_empty_payload() {
        let lyrics = lyrics_not_found_as_empty(
            AppError::UpstreamHttp {
                status: HttpStatusCode::NOT_FOUND,
                message: "apple api request failed: 404 Not Found".into(),
                retry_after: None,
            },
            ResponseFormat::Json,
        )
        .expect("empty lyrics");
        assert!(lyrics.is_empty());
    }

    #[test]
    fn error_response_uses_wrapped_subsonic_shape() {
        let response = subsonic_error_response(ResponseFormat::Json, 40, "bad auth");
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[test]
    fn xml_attr_escape_handles_special_characters() {
        assert_eq!(escape_xml_attr("a&b\"<>'"), "a&amp;b&quot;&lt;&gt;&apos;");
    }
}
