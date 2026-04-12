use std::collections::HashSet;
use std::sync::Arc;

use apple_music_api::{Artwork, SearchRequest, SongPlaybackMetadata};
use apple_music_decryptor::{
    ArtworkDescriptor, PlaybackRequest, PlaybackTrackMetadata, download_playback,
};
use axum::extract::{Query, Request, State};
use axum::http::header::{CONTENT_TYPE, HeaderValue};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use md5::Digest;
use serde::Deserialize;
use serde_json::{Value, json};
use tower::ServiceExt;
use tower_http::services::ServeFile;

use super::DaemonContext;
use crate::error::AppError;

const SUBSONIC_API_VERSION: &str = "1.16.1";
const SUBSONIC_SERVER_TYPE: &str = "wrapper-rs";
const SUBSONIC_MUSIC_FOLDER_ID: i32 = 1;

pub(super) fn router(context: Arc<DaemonContext>) -> Router<Arc<DaemonContext>> {
    Router::new()
        .route("/rest/ping.view", get(ping_handler))
        .route("/rest/getLicense.view", get(get_license_handler))
        .route("/rest/getMusicFolders.view", get(get_music_folders_handler))
        .route("/rest/getArtists.view", get(get_artists_handler))
        .route("/rest/getIndexes.view", get(get_indexes_handler))
        .route("/rest/search3.view", get(search3_handler))
        .route("/rest/getArtist.view", get(get_artist_handler))
        .route("/rest/getAlbum.view", get(get_album_handler))
        .route("/rest/getSong.view", get(get_song_handler))
        .route("/rest/getLyrics.view", get(get_lyrics_handler))
        .route("/rest/getCoverArt.view", get(get_cover_art_handler))
        .route("/rest/stream.view", get(stream_handler))
        .layer(middleware::from_fn_with_state(
            context,
            require_subsonic_auth,
        ))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResponseFormat {
    Json,
    Xml,
}

impl ResponseFormat {
    fn from_query(raw: Option<&str>) -> Result<Self, &'static str> {
        match raw.unwrap_or("xml").trim().to_ascii_lowercase().as_str() {
            "" | "xml" => Ok(Self::Xml),
            "json" => Ok(Self::Json),
            _ => Err("unsupported response format"),
        }
    }
}

#[derive(Debug, Deserialize)]
struct AuthQuery {
    u: Option<String>,
    p: Option<String>,
    t: Option<String>,
    s: Option<String>,
    v: Option<String>,
    c: Option<String>,
    f: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Search3Query {
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
struct IdQuery {
    #[serde(flatten)]
    auth: AuthQuery,
    id: String,
}

#[derive(Debug, Deserialize)]
struct CoverArtQuery {
    #[serde(flatten)]
    auth: AuthQuery,
    id: String,
    size: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct StreamQuery {
    #[serde(flatten)]
    auth: AuthQuery,
    id: String,
    #[serde(rename = "maxBitRate")]
    max_bit_rate: Option<u32>,
    format: Option<String>,
}

#[derive(Debug, Deserialize)]
struct LyricsQuery {
    #[serde(flatten)]
    auth: AuthQuery,
    artist: Option<String>,
    title: Option<String>,
    id: Option<String>,
}

#[derive(Clone)]
struct SubsonicArtist {
    id: String,
    name: String,
    cover_art: Option<String>,
    artist_image_url: Option<String>,
    album_count: Option<usize>,
}

#[derive(Clone)]
struct SubsonicAlbum {
    id: String,
    name: String,
    artist: String,
    artist_id: Option<String>,
    cover_art: Option<String>,
    song_count: Option<usize>,
    duration: Option<u64>,
    year: Option<i32>,
    created: Option<String>,
    genre: Option<String>,
}

#[derive(Clone)]
struct SubsonicSong {
    id: String,
    parent: Option<String>,
    title: String,
    album: Option<String>,
    artist: String,
    artist_id: Option<String>,
    cover_art: Option<String>,
    duration: Option<u64>,
    track: Option<u32>,
    disc_number: Option<u32>,
    year: Option<i32>,
    created: Option<String>,
    genre: Option<String>,
    suffix: &'static str,
    content_type: &'static str,
    album_id: Option<String>,
}

struct SubsonicLyrics {
    artist: String,
    title: String,
    value: String,
}

struct SubsonicError {
    format: ResponseFormat,
    code: i32,
    message: String,
}

impl SubsonicError {
    fn generic(format: ResponseFormat, message: impl Into<String>) -> Self {
        Self {
            format,
            code: 0,
            message: message.into(),
        }
    }

    fn required_parameter(format: ResponseFormat, message: impl Into<String>) -> Self {
        Self {
            format,
            code: 10,
            message: message.into(),
        }
    }

    fn authentication(format: ResponseFormat, message: impl Into<String>) -> Self {
        Self {
            format,
            code: 40,
            message: message.into(),
        }
    }

    fn not_found(format: ResponseFormat, message: impl Into<String>) -> Self {
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

async fn require_subsonic_auth(
    State(context): State<Arc<DaemonContext>>,
    request: Request,
    next: Next,
) -> Result<Response, SubsonicError> {
    let query = parse_auth_query(request.uri().query())?;
    validate_auth(&context, &query)?;
    Ok(next.run(request).await)
}

async fn ping_handler(Query(query): Query<AuthQuery>) -> Result<Response, SubsonicError> {
    Ok(subsonic_ok_response(response_format(&query)?, None, ""))
}

async fn get_license_handler(Query(query): Query<AuthQuery>) -> Result<Response, SubsonicError> {
    let format = response_format(&query)?;
    Ok(match format {
        ResponseFormat::Json => subsonic_ok_json(json!({
            "license": { "valid": true }
        })),
        ResponseFormat::Xml => subsonic_ok_xml(r#"<license valid="true"/>"#),
    })
}

async fn get_music_folders_handler(
    Query(query): Query<AuthQuery>,
) -> Result<Response, SubsonicError> {
    let format = response_format(&query)?;
    Ok(match format {
        ResponseFormat::Json => subsonic_ok_json(json!({
            "musicFolders": {
                "musicFolder": [
                    {
                        "id": SUBSONIC_MUSIC_FOLDER_ID,
                        "name": "Apple Music"
                    }
                ]
            }
        })),
        ResponseFormat::Xml => subsonic_ok_xml(&format!(
            r#"<musicFolders><musicFolder id="{SUBSONIC_MUSIC_FOLDER_ID}" name="Apple Music"/></musicFolders>"#
        )),
    })
}

async fn get_artists_handler(Query(query): Query<AuthQuery>) -> Result<Response, SubsonicError> {
    Err(SubsonicError::generic(
        response_format(&query)?,
        "Apple Music catalog does not expose server-side artist enumeration; use search3 instead",
    ))
}

async fn get_indexes_handler(Query(query): Query<AuthQuery>) -> Result<Response, SubsonicError> {
    Err(SubsonicError::generic(
        response_format(&query)?,
        "Apple Music catalog does not expose server-side artist enumeration; use search3 instead",
    ))
}

async fn search3_handler(
    State(context): State<Arc<DaemonContext>>,
    Query(query): Query<Search3Query>,
) -> Result<Response, SubsonicError> {
    let format = response_format(&query.auth)?;
    if query.query.trim().is_empty() {
        return Err(SubsonicError::required_parameter(
            format,
            "query is required",
        ));
    }

    let storefront = context.default_storefront();
    let artist_hits = context
        .api
        .search(SearchRequest {
            storefront,
            language: context.default_language(),
            query: &query.query,
            search_type: "artist",
            limit: query.artist_count.unwrap_or(20),
            offset: query.artist_offset.unwrap_or(0),
        })
        .await
        .map_err(|error| map_app_error(format, error.into()))?;
    let album_hits = context
        .api
        .search(SearchRequest {
            storefront,
            language: context.default_language(),
            query: &query.query,
            search_type: "album",
            limit: query.album_count.unwrap_or(20),
            offset: query.album_offset.unwrap_or(0),
        })
        .await
        .map_err(|error| map_app_error(format, error.into()))?;
    let song_hits = context
        .api
        .search(SearchRequest {
            storefront,
            language: context.default_language(),
            query: &query.query,
            search_type: "song",
            limit: query.song_count.unwrap_or(20),
            offset: query.song_offset.unwrap_or(0),
        })
        .await
        .map_err(|error| map_app_error(format, error.into()))?;

    let artists = search_results(&artist_hits, "/results/artists/data")
        .iter()
        .map(search_artist_to_subsonic)
        .collect::<Vec<_>>();
    let albums = search_results(&album_hits, "/results/albums/data")
        .iter()
        .map(search_album_to_subsonic)
        .collect::<Vec<_>>();
    let songs = search_results(&song_hits, "/results/songs/data")
        .iter()
        .map(search_song_to_subsonic)
        .collect::<Vec<_>>();

    Ok(match format {
        ResponseFormat::Json => subsonic_ok_json(json!({
            "searchResult3": {
                "artist": artists.iter().map(artist_json).collect::<Vec<_>>(),
                "album": albums.iter().map(album_json).collect::<Vec<_>>(),
                "song": songs.iter().map(song_json).collect::<Vec<_>>(),
            }
        })),
        ResponseFormat::Xml => {
            let artists_xml = artists.iter().map(artist_xml).collect::<String>();
            let albums_xml = albums.iter().map(album_xml).collect::<String>();
            let songs_xml = songs.iter().map(song_xml).collect::<String>();
            subsonic_ok_xml(&format!(
                "<searchResult3>{artists_xml}{albums_xml}{songs_xml}</searchResult3>"
            ))
        }
    })
}

async fn get_artist_handler(
    State(context): State<Arc<DaemonContext>>,
    Query(query): Query<IdQuery>,
) -> Result<Response, SubsonicError> {
    let format = response_format(&query.auth)?;
    let session = context
        .session()
        .map_err(|error| map_app_error(format, error))?;
    let profile = session.account_profile();
    let artist = context
        .api
        .artist(
            context.default_storefront(),
            context.default_language(),
            &profile.dev_token,
            &query.id,
            Some("full-albums,singles,latest-release"),
            Some(200),
        )
        .await
        .map_err(|error| map_app_error(format, error.into()))?;

    let artist_name = string_at(&artist, "/data/0/attributes/name")
        .ok_or_else(|| SubsonicError::not_found(format, "artist did not contain a name"))?;
    let artist_art =
        artwork_template(&artist, "/data/0/attributes/artwork/url").map(|_| query.id.clone());
    let artist_image_url = artwork_template(&artist, "/data/0/attributes/artwork/url")
        .map(|template| render_artwork_url(template, None));

    let mut seen_albums = HashSet::new();
    let mut albums = Vec::new();
    for path in [
        "/data/0/views/full-albums/data",
        "/data/0/views/singles/data",
        "/data/0/views/latest-release/data",
    ] {
        for item in search_results(&artist, path) {
            let album = search_album_to_subsonic(item);
            if seen_albums.insert(album.id.clone()) {
                albums.push(album);
            }
        }
    }

    let payload_artist = SubsonicArtist {
        id: query.id.clone(),
        name: artist_name.to_owned(),
        cover_art: artist_art,
        artist_image_url,
        album_count: Some(albums.len()),
    };
    Ok(match format {
        ResponseFormat::Json => subsonic_ok_json(json!({
            "artist": {
                **artist_json(&payload_artist).as_object().expect("artist json object"),
                "album": albums.iter().map(album_json).collect::<Vec<_>>(),
            }
        })),
        ResponseFormat::Xml => {
            let album_xml = albums.iter().map(album_xml).collect::<String>();
            subsonic_ok_xml(&format!(
                "<artist{}>{album_xml}</artist>",
                artist_attrs(&payload_artist)
            ))
        }
    })
}

async fn get_album_handler(
    State(context): State<Arc<DaemonContext>>,
    Query(query): Query<IdQuery>,
) -> Result<Response, SubsonicError> {
    let format = response_format(&query.auth)?;
    let session = context
        .session()
        .map_err(|error| map_app_error(format, error))?;
    let profile = session.account_profile();
    let album = context
        .api
        .album(
            context.default_storefront(),
            context.default_language(),
            &profile.dev_token,
            &query.id,
        )
        .await
        .map_err(|error| map_app_error(format, error.into()))?;

    let mut album_summary = album_detail_to_subsonic(&album)
        .ok_or_else(|| SubsonicError::not_found(format, "album did not return any data"))?;
    let songs = search_results(&album, "/data/0/relationships/tracks/data")
        .iter()
        .map(album_track_to_subsonic)
        .collect::<Vec<_>>();
    album_summary.duration = Some(songs.iter().filter_map(|song| song.duration).sum());
    album_summary.song_count = Some(songs.len());

    Ok(match format {
        ResponseFormat::Json => subsonic_ok_json(json!({
            "album": {
                **album_json(&album_summary).as_object().expect("album json object"),
                "song": songs.iter().map(song_json).collect::<Vec<_>>(),
            }
        })),
        ResponseFormat::Xml => {
            let songs_xml = songs.iter().map(song_xml).collect::<String>();
            subsonic_ok_xml(&format!(
                "<album{}>{songs_xml}</album>",
                album_attrs(&album_summary)
            ))
        }
    })
}

async fn get_song_handler(
    State(context): State<Arc<DaemonContext>>,
    Query(query): Query<IdQuery>,
) -> Result<Response, SubsonicError> {
    let format = response_format(&query.auth)?;
    let song = load_song(&context, &query.id, format).await?;
    Ok(match format {
        ResponseFormat::Json => subsonic_ok_json(json!({
            "song": song_json(&song)
        })),
        ResponseFormat::Xml => subsonic_ok_xml(&format!("<song{}/>", song_attrs(&song))),
    })
}

async fn get_lyrics_handler(
    State(context): State<Arc<DaemonContext>>,
    Query(query): Query<LyricsQuery>,
) -> Result<Response, SubsonicError> {
    let format = response_format(&query.auth)?;
    let song_id = if let Some(song_id) = query.id.as_deref().filter(|value| !value.is_empty()) {
        song_id.to_owned()
    } else {
        resolve_lyrics_song_id(
            &context,
            query.artist.as_deref(),
            query.title.as_deref(),
            format,
        )
        .await?
    };
    let song = load_song(&context, &song_id, format).await?;
    let lyrics_value = load_lyrics(&context, &song_id, format).await?;
    let lyrics = SubsonicLyrics {
        artist: song.artist.clone(),
        title: song.title.clone(),
        value: lyrics_value,
    };
    Ok(match format {
        ResponseFormat::Json => subsonic_ok_json(json!({
            "lyrics": {
                "artist": lyrics.artist,
                "title": lyrics.title,
                "value": lyrics.value,
            }
        })),
        ResponseFormat::Xml => subsonic_ok_xml(&format!(
            "<lyrics artist=\"{}\" title=\"{}\">{}</lyrics>",
            escape_xml_attr(&lyrics.artist),
            escape_xml_attr(&lyrics.title),
            escape_xml_text(&lyrics.value),
        )),
    })
}

async fn get_cover_art_handler(
    State(context): State<Arc<DaemonContext>>,
    Query(query): Query<CoverArtQuery>,
) -> Result<Response, SubsonicError> {
    let format = response_format(&query.auth)?;
    let session = context
        .session()
        .map_err(|error| map_app_error(format, error))?;
    let profile = session.account_profile();
    let artwork =
        match resolve_artwork(&context, &profile.dev_token, &query.id, query.size, format).await? {
            Some(artwork) => artwork,
            None => return Err(SubsonicError::not_found(format, "cover art not found")),
        };
    let mut client_builder = reqwest::Client::builder();
    if let Some(proxy) = context.config.proxy.as_deref() {
        client_builder = client_builder.proxy(
            reqwest::Proxy::all(proxy)
                .map_err(|error| SubsonicError::generic(format, error.to_string()))?,
        );
    }
    let client = client_builder
        .build()
        .map_err(|error| SubsonicError::generic(format, error.to_string()))?;
    let response = client
        .get(&artwork.url)
        .send()
        .await
        .map_err(|error| SubsonicError::generic(format, error.to_string()))?;
    if !response.status().is_success() {
        return Err(SubsonicError::not_found(
            format,
            format!("cover art request failed: {}", response.status()),
        ));
    }
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("image/jpeg")
        .to_owned();
    let bytes = response
        .bytes()
        .await
        .map_err(|error| SubsonicError::generic(format, error.to_string()))?;

    Ok((
        [(
            CONTENT_TYPE,
            HeaderValue::from_str(&content_type).unwrap_or(HeaderValue::from_static("image/jpeg")),
        )],
        bytes,
    )
        .into_response())
}

async fn stream_handler(
    State(context): State<Arc<DaemonContext>>,
    Query(query): Query<StreamQuery>,
    request: Request,
) -> Result<Response, SubsonicError> {
    let format = response_format(&query.auth)?;
    let session = context
        .session()
        .map_err(|error| map_app_error(format, error))?;
    let profile = session.account_profile();
    let metadata = context
        .api
        .song_playback_metadata(
            context.default_storefront(),
            context.default_language(),
            &profile.dev_token,
            &query.id,
        )
        .await
        .map_err(|error| map_app_error(format, error.into()))?;
    let lyrics = load_lyrics_optional(&context, &query.id).await;
    let config = context.config.download_config();
    let playback = tokio::task::spawn_blocking(move || {
        download_playback(
            config,
            session,
            PlaybackRequest {
                metadata: playback_track_metadata(metadata, lyrics),
                requested_codec: requested_codec(
                    query.max_bit_rate,
                    query.format.as_deref(),
                    format,
                )?,
            },
        )
    })
    .await
    .map_err(|error| SubsonicError::generic(format, format!("playback task panicked: {error}")))?
    .map_err(|error| map_app_error(format, error.into()))?;

    let file_path = context.config.cache_dir.join(
        playback
            .relative_path
            .strip_prefix("cache/")
            .unwrap_or(&playback.relative_path),
    );
    ServeFile::new(file_path)
        .oneshot(request)
        .await
        .map_err(|error| SubsonicError::generic(format, error.to_string()))
}

fn parse_auth_query(raw_query: Option<&str>) -> Result<AuthQuery, SubsonicError> {
    let format = raw_query
        .and_then(|query| serde_urlencoded::from_str::<AuthQuery>(query).ok())
        .map(|query| query.f.clone())
        .and_then(|value| ResponseFormat::from_query(value.as_deref()).ok())
        .unwrap_or(ResponseFormat::Xml);
    serde_urlencoded::from_str(raw_query.unwrap_or("")).map_err(|error| {
        SubsonicError::required_parameter(format, format!("invalid query string: {error}"))
    })
}

fn response_format(query: &AuthQuery) -> Result<ResponseFormat, SubsonicError> {
    ResponseFormat::from_query(query.f.as_deref())
        .map_err(|message| SubsonicError::required_parameter(ResponseFormat::Xml, message))
}

fn validate_auth(context: &DaemonContext, query: &AuthQuery) -> Result<(), SubsonicError> {
    let format = response_format(query)?;
    let username = query
        .u
        .as_deref()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| SubsonicError::required_parameter(format, "u is required"))?;
    if query
        .v
        .as_deref()
        .filter(|value| !value.is_empty())
        .is_none()
    {
        return Err(SubsonicError::required_parameter(format, "v is required"));
    }
    if query
        .c
        .as_deref()
        .filter(|value| !value.is_empty())
        .is_none()
    {
        return Err(SubsonicError::required_parameter(format, "c is required"));
    }
    if username != context.subsonic_username() {
        return Err(SubsonicError::authentication(
            format,
            "wrong username or password",
        ));
    }

    let authenticated = if let Some(password) = query.p.as_deref() {
        decode_password(password).as_deref() == Ok(context.subsonic_password())
    } else if let (Some(token), Some(salt)) = (query.t.as_deref(), query.s.as_deref()) {
        let digest = md5::compute(format!("{}{}", context.subsonic_password(), salt));
        format!("{:x}", digest) == token.to_ascii_lowercase()
    } else {
        return Err(SubsonicError::required_parameter(
            format,
            "either p or t+s is required",
        ));
    };

    if authenticated {
        Ok(())
    } else {
        Err(SubsonicError::authentication(
            format,
            "wrong username or password",
        ))
    }
}

fn decode_password(password: &str) -> Result<&str, SubsonicError> {
    if let Some(encoded) = password.strip_prefix("enc:") {
        let decoded = hex::decode(encoded).map_err(|_| {
            SubsonicError::authentication(ResponseFormat::Xml, "invalid encoded password")
        })?;
        let text = String::from_utf8(decoded).map_err(|_| {
            SubsonicError::authentication(ResponseFormat::Xml, "invalid encoded password")
        })?;
        Ok(Box::leak(text.into_boxed_str()))
    } else {
        Ok(password)
    }
}

fn map_app_error(format: ResponseFormat, error: AppError) -> SubsonicError {
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

fn search_results<'a>(value: &'a Value, pointer: &str) -> &'a [Value] {
    value
        .pointer(pointer)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

fn string_at<'a>(value: &'a Value, pointer: &str) -> Option<&'a str> {
    value.pointer(pointer).and_then(Value::as_str)
}

fn u64_at(value: &Value, pointer: &str) -> Option<u64> {
    value.pointer(pointer).and_then(Value::as_u64)
}

fn u32_at(value: &Value, pointer: &str) -> Option<u32> {
    u64_at(value, pointer).and_then(|value| u32::try_from(value).ok())
}

fn created_at(value: &Value, pointer: &str) -> Option<String> {
    string_at(value, pointer).map(|date| format!("{date}T00:00:00"))
}

fn release_year(value: &Value, pointer: &str) -> Option<i32> {
    string_at(value, pointer)
        .and_then(|date| date.get(..4))
        .and_then(|year| year.parse::<i32>().ok())
}

fn first_genre(value: &Value, pointer: &str) -> Option<String> {
    value
        .pointer(pointer)
        .and_then(Value::as_array)
        .and_then(|genres| genres.first())
        .and_then(Value::as_str)
        .map(str::to_owned)
}

fn artwork_template<'a>(value: &'a Value, pointer: &str) -> Option<&'a str> {
    string_at(value, pointer).filter(|value| !value.is_empty())
}

fn render_artwork_url(template: &str, size: Option<u32>) -> String {
    let width = size.unwrap_or(1200).max(1);
    let height = size.unwrap_or(width).max(1);
    template
        .replace("{w}", &width.to_string())
        .replace("{h}", &height.to_string())
}

fn search_artist_to_subsonic(value: &Value) -> SubsonicArtist {
    let id = string_at(value, "/id").unwrap_or_default().to_owned();
    let name = string_at(value, "/attributes/name")
        .unwrap_or_default()
        .to_owned();
    let has_artwork = artwork_template(value, "/attributes/artwork/url").is_some();
    SubsonicArtist {
        id: id.clone(),
        name,
        cover_art: has_artwork.then_some(id.clone()),
        artist_image_url: artwork_template(value, "/attributes/artwork/url")
            .map(|template| render_artwork_url(template, None)),
        album_count: None,
    }
}

fn search_album_to_subsonic(value: &Value) -> SubsonicAlbum {
    let id = string_at(value, "/id").unwrap_or_default().to_owned();
    SubsonicAlbum {
        id: id.clone(),
        name: string_at(value, "/attributes/name")
            .unwrap_or_default()
            .to_owned(),
        artist: string_at(value, "/attributes/artistName")
            .unwrap_or_default()
            .to_owned(),
        artist_id: None,
        cover_art: artwork_template(value, "/attributes/artwork/url").map(|_| id),
        song_count: u64_at(value, "/attributes/trackCount")
            .and_then(|count| usize::try_from(count).ok()),
        duration: None,
        year: release_year(value, "/attributes/releaseDate"),
        created: created_at(value, "/attributes/releaseDate"),
        genre: first_genre(value, "/attributes/genreNames"),
    }
}

fn search_song_to_subsonic(value: &Value) -> SubsonicSong {
    let album_id = string_at(value, "/relationships/albums/data/0/id").map(str::to_owned);
    let cover_art = album_id.clone().or_else(|| {
        artwork_template(value, "/attributes/artwork/url")
            .is_some()
            .then(|| string_at(value, "/id").unwrap_or_default().to_owned())
    });
    SubsonicSong {
        id: string_at(value, "/id").unwrap_or_default().to_owned(),
        parent: album_id.clone(),
        title: string_at(value, "/attributes/name")
            .unwrap_or_default()
            .to_owned(),
        album: string_at(value, "/attributes/albumName").map(str::to_owned),
        artist: string_at(value, "/attributes/artistName")
            .unwrap_or_default()
            .to_owned(),
        artist_id: string_at(value, "/relationships/artists/data/0/id").map(str::to_owned),
        cover_art,
        duration: u64_at(value, "/attributes/durationInMillis").map(|millis| millis / 1000),
        track: u32_at(value, "/attributes/trackNumber"),
        disc_number: u32_at(value, "/attributes/discNumber"),
        year: release_year(value, "/attributes/releaseDate"),
        created: created_at(value, "/attributes/releaseDate"),
        genre: first_genre(value, "/attributes/genreNames"),
        suffix: "m4a",
        content_type: "audio/mp4",
        album_id,
    }
}

fn album_detail_to_subsonic(value: &Value) -> Option<SubsonicAlbum> {
    let id = string_at(value, "/data/0/id")?.to_owned();
    Some(SubsonicAlbum {
        id: id.clone(),
        name: string_at(value, "/data/0/attributes/name")?.to_owned(),
        artist: string_at(value, "/data/0/attributes/artistName")
            .unwrap_or_default()
            .to_owned(),
        artist_id: string_at(value, "/data/0/relationships/artists/data/0/id").map(str::to_owned),
        cover_art: artwork_template(value, "/data/0/attributes/artwork/url").map(|_| id),
        song_count: search_results(value, "/data/0/relationships/tracks/data")
            .len()
            .into(),
        duration: None,
        year: release_year(value, "/data/0/attributes/releaseDate"),
        created: created_at(value, "/data/0/attributes/releaseDate"),
        genre: first_genre(value, "/data/0/attributes/genreNames"),
    })
}

fn album_track_to_subsonic(value: &Value) -> SubsonicSong {
    let album_id = string_at(value, "/attributes/playParams/catalogId")
        .or_else(|| string_at(value, "/relationships/albums/data/0/id"))
        .map(str::to_owned);
    SubsonicSong {
        id: string_at(value, "/id").unwrap_or_default().to_owned(),
        parent: album_id.clone(),
        title: string_at(value, "/attributes/name")
            .unwrap_or_default()
            .to_owned(),
        album: string_at(value, "/attributes/albumName").map(str::to_owned),
        artist: string_at(value, "/attributes/artistName")
            .unwrap_or_default()
            .to_owned(),
        artist_id: string_at(value, "/relationships/artists/data/0/id").map(str::to_owned),
        cover_art: album_id.clone(),
        duration: u64_at(value, "/attributes/durationInMillis").map(|millis| millis / 1000),
        track: u32_at(value, "/attributes/trackNumber"),
        disc_number: u32_at(value, "/attributes/discNumber"),
        year: release_year(value, "/attributes/releaseDate"),
        created: created_at(value, "/attributes/releaseDate"),
        genre: first_genre(value, "/attributes/genreNames"),
        suffix: "m4a",
        content_type: "audio/mp4",
        album_id,
    }
}

async fn load_song(
    context: &DaemonContext,
    song_id: &str,
    format: ResponseFormat,
) -> Result<SubsonicSong, SubsonicError> {
    let session = context
        .session()
        .map_err(|error| map_app_error(format, error))?;
    let profile = session.account_profile();
    let song = context
        .api
        .song(
            context.default_storefront(),
            context.default_language(),
            &profile.dev_token,
            song_id,
        )
        .await
        .map_err(|error| map_app_error(format, error.into()))?;
    let item = search_results(&song, "/data").first().ok_or_else(|| {
        SubsonicError::not_found(format, format!("song {song_id} did not return any data"))
    })?;
    Ok(search_song_to_subsonic(item))
}

async fn load_lyrics(
    context: &DaemonContext,
    song_id: &str,
    format: ResponseFormat,
) -> Result<String, SubsonicError> {
    let session = context
        .session()
        .map_err(|error| map_app_error(format, error))?;
    let profile = session.account_profile();
    context
        .api
        .lyrics(
            context.default_storefront(),
            context.default_language(),
            &profile.dev_token,
            &profile.music_token,
            song_id,
        )
        .await
        .map_err(|error| map_app_error(format, error.into()))
}

async fn load_lyrics_optional(context: &DaemonContext, song_id: &str) -> Option<String> {
    let session = context.session().ok()?;
    let profile = session.account_profile();
    context
        .api
        .lyrics(
            context.default_storefront(),
            context.default_language(),
            &profile.dev_token,
            &profile.music_token,
            song_id,
        )
        .await
        .ok()
}

async fn resolve_lyrics_song_id(
    context: &DaemonContext,
    artist: Option<&str>,
    title: Option<&str>,
    format: ResponseFormat,
) -> Result<String, SubsonicError> {
    let title = title
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| SubsonicError::required_parameter(format, "title is required"))?;
    let search = context
        .api
        .search(SearchRequest {
            storefront: context.default_storefront(),
            language: context.default_language(),
            query: title,
            search_type: "song",
            limit: 20,
            offset: 0,
        })
        .await
        .map_err(|error| map_app_error(format, error.into()))?;
    let requested_artist = artist.map(normalize_match_text);
    let requested_title = normalize_match_text(title);
    let songs = search_results(&search, "/results/songs/data");
    let exact = songs.iter().find(|item| {
        normalize_match_text(string_at(item, "/attributes/name").unwrap_or_default())
            == requested_title
            && requested_artist.as_ref().is_none_or(|artist| {
                normalize_match_text(string_at(item, "/attributes/artistName").unwrap_or_default())
                    == *artist
            })
    });
    let fallback = songs.iter().find(|item| {
        normalize_match_text(string_at(item, "/attributes/name").unwrap_or_default())
            == requested_title
    });
    exact
        .or(fallback)
        .and_then(|item| string_at(item, "/id"))
        .map(str::to_owned)
        .ok_or_else(|| SubsonicError::not_found(format, "lyrics target song not found"))
}

async fn resolve_artwork(
    context: &DaemonContext,
    dev_token: &str,
    item_id: &str,
    size: Option<u32>,
    format: ResponseFormat,
) -> Result<Option<ArtworkDescriptor>, SubsonicError> {
    if let Ok(song) = context
        .api
        .song(
            context.default_storefront(),
            context.default_language(),
            dev_token,
            item_id,
        )
        .await
    {
        if let Some(template) = artwork_template(&song, "/data/0/attributes/artwork/url") {
            return Ok(Some(ArtworkDescriptor {
                url: render_artwork_url(template, size),
                width: size,
                height: size,
            }));
        }
        if let Some(template) = artwork_template(
            &song,
            "/data/0/relationships/albums/data/0/attributes/artwork/url",
        ) {
            return Ok(Some(ArtworkDescriptor {
                url: render_artwork_url(template, size),
                width: size,
                height: size,
            }));
        }
    }

    if let Ok(album) = context
        .api
        .album(
            context.default_storefront(),
            context.default_language(),
            dev_token,
            item_id,
        )
        .await
        && let Some(template) = artwork_template(&album, "/data/0/attributes/artwork/url")
    {
        return Ok(Some(ArtworkDescriptor {
            url: render_artwork_url(template, size),
            width: size,
            height: size,
        }));
    }

    if let Ok(artist) = context
        .api
        .artist(
            context.default_storefront(),
            context.default_language(),
            dev_token,
            item_id,
            Some("latest-release"),
            Some(1),
        )
        .await
    {
        if let Some(template) = artwork_template(&artist, "/data/0/attributes/artwork/url") {
            return Ok(Some(ArtworkDescriptor {
                url: render_artwork_url(template, size),
                width: size,
                height: size,
            }));
        }
        if let Some(template) = artwork_template(
            &artist,
            "/data/0/views/latest-release/data/0/attributes/artwork/url",
        ) {
            return Ok(Some(ArtworkDescriptor {
                url: render_artwork_url(template, size),
                width: size,
                height: size,
            }));
        }
    }

    let _ = format;
    Ok(None)
}

fn requested_codec(
    max_bit_rate: Option<u32>,
    format: Option<&str>,
    response_format: ResponseFormat,
) -> Result<Option<String>, SubsonicError> {
    let format = format.unwrap_or("").trim().to_ascii_lowercase();
    let codec = match format.as_str() {
        "" | "raw" | "m4a" | "mp4" => {
            if max_bit_rate.is_some() {
                Some("aac".to_owned())
            } else {
                None
            }
        }
        "aac" => Some("aac".to_owned()),
        "alac" | "lossless" => Some("alac".to_owned()),
        other => {
            return Err(SubsonicError::required_parameter(
                response_format,
                format!("unsupported stream format: {other}"),
            ));
        }
    };
    Ok(codec)
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
        url: render_artwork_url(&artwork.url, None),
        width: artwork.width,
        height: artwork.height,
    }
}

fn normalize_match_text(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}

fn subsonic_ok_response(
    format: ResponseFormat,
    json_body: Option<Value>,
    xml_fragment: &str,
) -> Response {
    match format {
        ResponseFormat::Json => subsonic_ok_json(json_body.unwrap_or_else(|| json!({}))),
        ResponseFormat::Xml => subsonic_ok_xml(xml_fragment),
    }
}

fn subsonic_ok_json(payload: Value) -> Response {
    let mut response = serde_json::Map::new();
    response.insert("status".into(), json!("ok"));
    response.insert("version".into(), json!(SUBSONIC_API_VERSION));
    response.insert("type".into(), json!(SUBSONIC_SERVER_TYPE));
    response.insert("serverVersion".into(), json!(crate::BUILD_VERSION));
    if let Some(object) = payload.as_object() {
        response.extend(object.clone());
    }
    Json(json!({
        "subsonic-response": Value::Object(response)
    }))
    .into_response()
}

fn subsonic_ok_xml(fragment: &str) -> Response {
    (
        [(CONTENT_TYPE, HeaderValue::from_static("application/xml; charset=utf-8"))],
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?><subsonic-response xmlns="http://subsonic.org/restapi" status="ok" version="{SUBSONIC_API_VERSION}" type="{SUBSONIC_SERVER_TYPE}" serverVersion="{}">{fragment}</subsonic-response>"#,
            crate::BUILD_VERSION,
        ),
    )
        .into_response()
}

fn subsonic_error_response(format: ResponseFormat, code: i32, message: &str) -> Response {
    match format {
        ResponseFormat::Json => Json(json!({
            "subsonic-response": {
                "status": "failed",
                "version": SUBSONIC_API_VERSION,
                "type": SUBSONIC_SERVER_TYPE,
                "serverVersion": crate::BUILD_VERSION,
                "error": {
                    "code": code,
                    "message": message,
                }
            }
        }))
        .into_response(),
        ResponseFormat::Xml => (
            [(CONTENT_TYPE, HeaderValue::from_static("application/xml; charset=utf-8"))],
            format!(
                r#"<?xml version="1.0" encoding="UTF-8"?><subsonic-response xmlns="http://subsonic.org/restapi" status="failed" version="{SUBSONIC_API_VERSION}" type="{SUBSONIC_SERVER_TYPE}" serverVersion="{}"><error code="{code}" message="{}"/></subsonic-response>"#,
                crate::BUILD_VERSION,
                escape_xml_attr(message),
            ),
        )
            .into_response(),
    }
}

fn artist_json(artist: &SubsonicArtist) -> Value {
    let mut value = serde_json::Map::new();
    value.insert("id".into(), json!(artist.id));
    value.insert("name".into(), json!(artist.name));
    if let Some(album_count) = artist.album_count {
        value.insert("albumCount".into(), json!(album_count));
    }
    if let Some(cover_art) = artist.cover_art.as_deref() {
        value.insert("coverArt".into(), json!(cover_art));
    }
    if let Some(image_url) = artist.artist_image_url.as_deref() {
        value.insert("artistImageUrl".into(), json!(image_url));
    }
    Value::Object(value)
}

fn album_json(album: &SubsonicAlbum) -> Value {
    let mut value = serde_json::Map::new();
    value.insert("id".into(), json!(album.id));
    value.insert("name".into(), json!(album.name));
    value.insert("artist".into(), json!(album.artist));
    value.insert("musicFolderId".into(), json!(SUBSONIC_MUSIC_FOLDER_ID));
    if let Some(artist_id) = album.artist_id.as_deref() {
        value.insert("artistId".into(), json!(artist_id));
    }
    if let Some(cover_art) = album.cover_art.as_deref() {
        value.insert("coverArt".into(), json!(cover_art));
    }
    if let Some(song_count) = album.song_count {
        value.insert("songCount".into(), json!(song_count));
    }
    if let Some(duration) = album.duration {
        value.insert("duration".into(), json!(duration));
    }
    if let Some(year) = album.year {
        value.insert("year".into(), json!(year));
    }
    if let Some(created) = album.created.as_deref() {
        value.insert("created".into(), json!(created));
    }
    if let Some(genre) = album.genre.as_deref() {
        value.insert("genre".into(), json!(genre));
    }
    Value::Object(value)
}

fn song_json(song: &SubsonicSong) -> Value {
    let mut value = serde_json::Map::new();
    value.insert("id".into(), json!(song.id));
    value.insert("isDir".into(), json!(false));
    value.insert("title".into(), json!(song.title));
    value.insert("artist".into(), json!(song.artist));
    value.insert("suffix".into(), json!(song.suffix));
    value.insert("contentType".into(), json!(song.content_type));
    value.insert("musicFolderId".into(), json!(SUBSONIC_MUSIC_FOLDER_ID));
    if let Some(parent) = song.parent.as_deref() {
        value.insert("parent".into(), json!(parent));
    }
    if let Some(album) = song.album.as_deref() {
        value.insert("album".into(), json!(album));
    }
    if let Some(artist_id) = song.artist_id.as_deref() {
        value.insert("artistId".into(), json!(artist_id));
    }
    if let Some(cover_art) = song.cover_art.as_deref() {
        value.insert("coverArt".into(), json!(cover_art));
    }
    if let Some(duration) = song.duration {
        value.insert("duration".into(), json!(duration));
    }
    if let Some(track) = song.track {
        value.insert("track".into(), json!(track));
    }
    if let Some(disc_number) = song.disc_number {
        value.insert("discNumber".into(), json!(disc_number));
    }
    if let Some(year) = song.year {
        value.insert("year".into(), json!(year));
    }
    if let Some(created) = song.created.as_deref() {
        value.insert("created".into(), json!(created));
    }
    if let Some(genre) = song.genre.as_deref() {
        value.insert("genre".into(), json!(genre));
    }
    if let Some(album_id) = song.album_id.as_deref() {
        value.insert("albumId".into(), json!(album_id));
    }
    Value::Object(value)
}

fn artist_xml(artist: &SubsonicArtist) -> String {
    format!("<artist{}/>", artist_attrs(artist))
}

fn artist_attrs(artist: &SubsonicArtist) -> String {
    let mut attrs = format!(
        r#" id="{}" name="{}""#,
        escape_xml_attr(&artist.id),
        escape_xml_attr(&artist.name),
    );
    if let Some(album_count) = artist.album_count {
        attrs.push_str(&format!(r#" albumCount="{album_count}""#));
    }
    if let Some(cover_art) = artist.cover_art.as_deref() {
        attrs.push_str(&format!(r#" coverArt="{}""#, escape_xml_attr(cover_art)));
    }
    if let Some(image_url) = artist.artist_image_url.as_deref() {
        attrs.push_str(&format!(
            r#" artistImageUrl="{}""#,
            escape_xml_attr(image_url)
        ));
    }
    attrs
}

fn album_xml(album: &SubsonicAlbum) -> String {
    format!("<album{}/>", album_attrs(album))
}

fn album_attrs(album: &SubsonicAlbum) -> String {
    let mut attrs = format!(
        r#" id="{}" name="{}" artist="{}" musicFolderId="{SUBSONIC_MUSIC_FOLDER_ID}""#,
        escape_xml_attr(&album.id),
        escape_xml_attr(&album.name),
        escape_xml_attr(&album.artist),
    );
    if let Some(artist_id) = album.artist_id.as_deref() {
        attrs.push_str(&format!(r#" artistId="{}""#, escape_xml_attr(artist_id)));
    }
    if let Some(cover_art) = album.cover_art.as_deref() {
        attrs.push_str(&format!(r#" coverArt="{}""#, escape_xml_attr(cover_art)));
    }
    if let Some(song_count) = album.song_count {
        attrs.push_str(&format!(r#" songCount="{song_count}""#));
    }
    if let Some(duration) = album.duration {
        attrs.push_str(&format!(r#" duration="{duration}""#));
    }
    if let Some(year) = album.year {
        attrs.push_str(&format!(r#" year="{year}""#));
    }
    if let Some(created) = album.created.as_deref() {
        attrs.push_str(&format!(r#" created="{}""#, escape_xml_attr(created)));
    }
    if let Some(genre) = album.genre.as_deref() {
        attrs.push_str(&format!(r#" genre="{}""#, escape_xml_attr(genre)));
    }
    attrs
}

fn song_xml(song: &SubsonicSong) -> String {
    format!("<song{}/>", song_attrs(song))
}

fn song_attrs(song: &SubsonicSong) -> String {
    let mut attrs = format!(
        r#" id="{}" isDir="false" title="{}" artist="{}" suffix="{}" contentType="{}" musicFolderId="{SUBSONIC_MUSIC_FOLDER_ID}""#,
        escape_xml_attr(&song.id),
        escape_xml_attr(&song.title),
        escape_xml_attr(&song.artist),
        song.suffix,
        song.content_type,
    );
    if let Some(parent) = song.parent.as_deref() {
        attrs.push_str(&format!(r#" parent="{}""#, escape_xml_attr(parent)));
    }
    if let Some(album) = song.album.as_deref() {
        attrs.push_str(&format!(r#" album="{}""#, escape_xml_attr(album)));
    }
    if let Some(artist_id) = song.artist_id.as_deref() {
        attrs.push_str(&format!(r#" artistId="{}""#, escape_xml_attr(artist_id)));
    }
    if let Some(cover_art) = song.cover_art.as_deref() {
        attrs.push_str(&format!(r#" coverArt="{}""#, escape_xml_attr(cover_art)));
    }
    if let Some(duration) = song.duration {
        attrs.push_str(&format!(r#" duration="{duration}""#));
    }
    if let Some(track) = song.track {
        attrs.push_str(&format!(r#" track="{track}""#));
    }
    if let Some(disc_number) = song.disc_number {
        attrs.push_str(&format!(r#" discNumber="{disc_number}""#));
    }
    if let Some(year) = song.year {
        attrs.push_str(&format!(r#" year="{year}""#));
    }
    if let Some(created) = song.created.as_deref() {
        attrs.push_str(&format!(r#" created="{}""#, escape_xml_attr(created)));
    }
    if let Some(genre) = song.genre.as_deref() {
        attrs.push_str(&format!(r#" genre="{}""#, escape_xml_attr(genre)));
    }
    if let Some(album_id) = song.album_id.as_deref() {
        attrs.push_str(&format!(r#" albumId="{}""#, escape_xml_attr(album_id)));
    }
    attrs
}

fn escape_xml_attr(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn escape_xml_text(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::{
        AuthQuery, ResponseFormat, escape_xml_attr, parse_auth_query, requested_codec,
        subsonic_error_response, validate_auth,
    };
    use crate::config::AppConfig;
    use crate::daemon::DaemonContext;
    use crate::runtime::AppState;
    use apple_music_decryptor::{DeviceInfo, NativePlatform};
    use axum::http::StatusCode;
    use std::path::PathBuf;
    use std::str::FromStr;
    use std::sync::Arc;

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
        let context = test_context();
        validate_auth(
            &context,
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

        validate_auth(
            &context,
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
    fn error_response_uses_wrapped_subsonic_shape() {
        let response = subsonic_error_response(ResponseFormat::Json, 40, "bad auth");
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[test]
    fn xml_attr_escape_handles_special_characters() {
        assert_eq!(escape_xml_attr("a&b\"<>'"), "a&amp;b&quot;&lt;&gt;&apos;");
    }

    fn test_context() -> DaemonContext {
        DaemonContext::new(
            AppConfig {
                host: std::net::IpAddr::from_str("127.0.0.1").expect("host"),
                daemon_port: 8080,
                api_token: "secret".into(),
                subsonic_username: "wrapper".into(),
                subsonic_password: "secret".into(),
                proxy: None,
                base_dir: PathBuf::from("."),
                library_dir: PathBuf::from("."),
                cache_dir: PathBuf::from("cache"),
                storefront: "us".into(),
                language: String::new(),
                device_info: DeviceInfo::parse(
                    "Music/4.9/Android/10/Samsung S9/7663313/en-US/en-US/dc28071e981c439e",
                )
                .expect("device info"),
                decrypt_workers: 2,
                decrypt_inflight: 4,
            },
            Arc::new(unsafe { std::mem::MaybeUninit::<NativePlatform>::zeroed().assume_init() }),
            Arc::new(AppState::default()),
        )
        .expect("context")
    }
}
