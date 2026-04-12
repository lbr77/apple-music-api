use std::collections::HashMap;
use std::sync::{Arc, LazyLock};
use std::time::{Duration, Instant};

use regex::Regex;
use reqwest::header::{AUTHORIZATION, COOKIE, ORIGIN, REFERER};
use reqwest::{Client, Proxy};
use roxmltree::Document;
use serde::Deserialize;
use serde_json::Value;
use tokio::sync::{Mutex, Notify, Semaphore};

use crate::error::{ApiResult, AppleMusicApiError};

const MUSIC_ORIGIN: &str = "https://music.apple.com";
const DESKTOP_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/91.0.4472.124 Safari/537.36";
const SEARCH_CACHE_TTL: Duration = Duration::from_secs(15);
const SEARCH_THROTTLE_WINDOW: Duration = Duration::from_millis(400);
const SEARCH_RATE_LIMIT_TTL: Duration = Duration::from_secs(2);
const SEARCH_MAX_CONCURRENCY: usize = 1;
const WEB_TOKEN_TTL: Duration = Duration::from_secs(30 * 60);
const DEFAULT_ARTIST_INCLUDE: &str = "genres,station";
const DEFAULT_ARTIST_VIEWS: &str = "top-songs,latest-release,full-albums,singles,featured-playlists,playlists,similar-artists,top-music-videos";
static INDEX_JS_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"/assets/index~[^/"']+\.js"#).expect("index js regex should compile")
});
static WEB_TOKEN_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"eyJh[^"']*"#).expect("web token regex should compile"));

pub struct SearchRequest<'a> {
    pub storefront: &'a str,
    pub language: Option<&'a str>,
    pub query: &'a str,
    pub search_type: &'a str,
    pub limit: usize,
    pub offset: usize,
}

pub struct ArtistViewRequest<'a> {
    pub storefront: &'a str,
    pub language: Option<&'a str>,
    pub dev_token: &'a str,
    pub artist_id: &'a str,
    pub view_name: &'a str,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

impl SearchRequest<'_> {
    fn cache_key(&self) -> SearchCacheKey {
        SearchCacheKey {
            storefront: self.storefront.to_owned(),
            language: self
                .language
                .filter(|value| !value.is_empty())
                .map(str::to_owned),
            query: self.query.trim().to_owned(),
            search_type: self.search_type.to_owned(),
            limit: self.limit,
            offset: self.offset,
        }
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct SearchCacheKey {
    storefront: String,
    language: Option<String>,
    query: String,
    search_type: String,
    limit: usize,
    offset: usize,
}

#[derive(Clone)]
struct SearchCacheEntry {
    expires_at: Instant,
    payload: CachedSearchPayload,
}

#[derive(Clone)]
enum CachedSearchPayload {
    Response(Value),
    Error(CachedUpstreamError),
}

impl CachedSearchPayload {
    fn into_result(self) -> ApiResult<Value> {
        match self {
            Self::Response(value) => Ok(value),
            Self::Error(error) => Err(error.into_app_error()),
        }
    }
}

#[derive(Clone)]
struct CachedUpstreamError {
    status: reqwest::StatusCode,
    message: String,
    retry_after: Option<String>,
}

struct WebTokenCacheEntry {
    token: String,
    expires_at: Instant,
}

impl CachedUpstreamError {
    fn into_app_error(self) -> AppleMusicApiError {
        AppleMusicApiError::UpstreamHttp {
            status: self.status,
            message: self.message,
            retry_after: self.retry_after,
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct Artwork {
    pub url: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct SongPlaybackMetadata {
    pub song_id: String,
    pub artist: String,
    pub artist_id: String,
    pub album_id: String,
    pub album: String,
    pub title: String,
    pub track_number: u32,
    pub disc_number: u32,
    pub artwork: Option<Artwork>,
    pub album_artwork: Option<Artwork>,
}

#[derive(Clone)]
pub struct AppleApiClient {
    client: Client,
    search_cache: Arc<Mutex<HashMap<SearchCacheKey, SearchCacheEntry>>>,
    search_inflight: Arc<Mutex<HashMap<SearchCacheKey, Arc<Notify>>>>,
    search_gate: Arc<Semaphore>,
    search_next_allowed_at: Arc<Mutex<Instant>>,
    web_token: Arc<Mutex<Option<WebTokenCacheEntry>>>,
}

impl AppleApiClient {
    pub fn new(proxy: Option<&str>) -> ApiResult<Self> {
        let mut builder = Client::builder().user_agent(DESKTOP_USER_AGENT);
        if let Some(proxy) = proxy {
            builder = builder.proxy(Proxy::all(proxy)?);
        }
        Ok(Self {
            client: builder.build()?,
            search_cache: Arc::new(Mutex::new(HashMap::new())),
            search_inflight: Arc::new(Mutex::new(HashMap::new())),
            search_gate: Arc::new(Semaphore::new(SEARCH_MAX_CONCURRENCY)),
            search_next_allowed_at: Arc::new(Mutex::new(Instant::now())),
            web_token: Arc::new(Mutex::new(None)),
        })
    }

    pub async fn search(&self, request: SearchRequest<'_>) -> ApiResult<Value> {
        let key = request.cache_key();

        loop {
            if let Some(cached) = self.cached_search_payload(&key).await {
                return cached.into_result();
            }

            let inflight = {
                let mut search_inflight = self.search_inflight.lock().await;
                if let Some(notify) = search_inflight.get(&key) {
                    Some(Arc::clone(notify))
                } else {
                    let notify = Arc::new(Notify::new());
                    search_inflight.insert(key.clone(), Arc::clone(&notify));
                    None
                }
            };

            if let Some(notify) = inflight {
                notify.notified().await;
                continue;
            }

            let result = self.search_apple_catalog(&request).await;

            match &result {
                Ok(value) => {
                    self.insert_search_cache(
                        key.clone(),
                        CachedSearchPayload::Response(value.clone()),
                        SEARCH_CACHE_TTL,
                    )
                    .await;
                }
                Err(AppleMusicApiError::UpstreamHttp {
                    status,
                    message,
                    retry_after,
                }) if *status == reqwest::StatusCode::TOO_MANY_REQUESTS => {
                    self.insert_search_cache(
                        key.clone(),
                        CachedSearchPayload::Error(CachedUpstreamError {
                            status: *status,
                            message: message.clone(),
                            retry_after: retry_after.clone(),
                        }),
                        retry_after_ttl(retry_after.as_deref()),
                    )
                    .await;
                }
                Err(_) => {}
            }

            self.finish_search_flight(&key).await;
            return result;
        }
    }

    pub async fn album(
        &self,
        storefront: &str,
        language: Option<&str>,
        dev_token: &str,
        album_id: &str,
    ) -> ApiResult<Value> {
        let mut album = self
            .catalog_json(
                format!("/v1/catalog/{storefront}/albums/{album_id}"),
                language,
                dev_token,
                None,
                &[
                    ("omit[resource]", "autos".into()),
                    ("include", "tracks,artists,record-labels".into()),
                    ("include[songs]", "artists".into()),
                    ("extend", "editorialVideo,extendedAssetUrls".into()),
                ],
            )
            .await?;

        while let Some(next_path) = album
            .pointer("/data/0/relationships/tracks/next")
            .and_then(Value::as_str)
            .map(str::to_owned)
        {
            let next_page = self
                .catalog_json(
                    next_path,
                    language,
                    dev_token,
                    None,
                    &[
                        ("omit[resource]", "autos".into()),
                        ("include", "artists".into()),
                        ("extend", "editorialVideo,extendedAssetUrls".into()),
                    ],
                )
                .await?;
            append_album_tracks(&mut album, next_page)?;
        }

        Ok(album)
    }

    pub async fn song(
        &self,
        storefront: &str,
        language: Option<&str>,
        dev_token: &str,
        song_id: &str,
    ) -> ApiResult<Value> {
        self.catalog_json(
            format!("/v1/catalog/{storefront}/songs/{song_id}"),
            language,
            dev_token,
            None,
            &[
                ("include", "albums,artists".into()),
                ("extend", "extendedAssetUrls".into()),
            ],
        )
        .await
    }

    pub async fn song_playback_metadata(
        &self,
        storefront: &str,
        language: Option<&str>,
        dev_token: &str,
        song_id: &str,
    ) -> ApiResult<SongPlaybackMetadata> {
        let response: SongPlaybackResponse = self
            .catalog(
                format!("/v1/catalog/{storefront}/songs/{song_id}"),
                language,
                dev_token,
                None,
                &[
                    ("include", "albums,artists".into()),
                    ("extend", "extendedAssetUrls".into()),
                ],
            )
            .await?;
        let song = response.data.into_iter().next().ok_or_else(|| {
            AppleMusicApiError::Message(format!("song {song_id} did not return any data"))
        })?;
        let album = song
            .relationships
            .albums
            .data
            .into_iter()
            .next()
            .ok_or_else(|| {
                AppleMusicApiError::Message(format!("song {song_id} is missing album metadata"))
            })?;
        let artist = song
            .relationships
            .artists
            .data
            .into_iter()
            .next()
            .ok_or_else(|| {
                AppleMusicApiError::Message(format!("song {song_id} is missing artist metadata"))
            })?;

        Ok(SongPlaybackMetadata {
            song_id: song_id.to_owned(),
            artist: song.attributes.artist_name,
            artist_id: artist.id,
            album_id: album.id,
            album: song.attributes.album_name,
            title: song.attributes.name,
            track_number: song.attributes.track_number,
            disc_number: song.attributes.disc_number,
            artwork: song.attributes.artwork,
            album_artwork: album.attributes.and_then(|attributes| attributes.artwork),
        })
    }

    pub async fn artist(
        &self,
        storefront: &str,
        language: Option<&str>,
        dev_token: &str,
        artist_id: &str,
        views: Option<&str>,
        limit: Option<usize>,
    ) -> ApiResult<Value> {
        let mut params = vec![("include", DEFAULT_ARTIST_INCLUDE.to_owned())];
        params.push((
            "views",
            views
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or(DEFAULT_ARTIST_VIEWS)
                .to_owned(),
        ));
        if let Some(limit) = limit {
            params.push(("limit", limit.to_string()));
        }

        self.catalog_json(
            format!("/v1/catalog/{storefront}/artists/{artist_id}"),
            language,
            dev_token,
            None,
            &params,
        )
        .await
    }

    pub async fn artist_view(&self, request: ArtistViewRequest<'_>) -> ApiResult<Value> {
        let mut params = Vec::new();
        if let Some(limit) = request.limit {
            params.push(("limit", limit.to_string()));
        }
        if let Some(offset) = request.offset {
            params.push(("offset", offset.to_string()));
        }

        self.catalog_json(
            format!(
                "/v1/catalog/{}/artists/{}/view/{}",
                request.storefront, request.artist_id, request.view_name
            ),
            request.language,
            request.dev_token,
            None,
            &params,
        )
        .await
    }

    pub async fn lyrics(
        &self,
        storefront: &str,
        language: Option<&str>,
        music_token: &str,
        song_id: &str,
    ) -> ApiResult<String> {
        let web_token = self.web_token(false).await?;
        let result = self
            .catalog_json(
                format!("/v1/catalog/{storefront}/songs/{song_id}/lyrics"),
                language,
                &web_token,
                Some(music_token),
                &[("extend", "ttmlLocalizations".into())],
            )
            .await;
        let response = if let Err(AppleMusicApiError::UpstreamHttp { status, .. }) = &result
            && matches!(
                *status,
                reqwest::StatusCode::UNAUTHORIZED | reqwest::StatusCode::FORBIDDEN
            ) {
            let refreshed_web_token = self.web_token(true).await?;
            self.catalog_json(
                format!("/v1/catalog/{storefront}/songs/{song_id}/lyrics"),
                language,
                &refreshed_web_token,
                Some(music_token),
                &[("extend", "ttmlLocalizations".into())],
            )
            .await?
        } else {
            result?
        };
        let ttml = response
            .pointer("/data/0/attributes/ttml")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .or_else(|| {
                response
                    .pointer("/data/0/attributes/ttmlLocalizations")
                    .and_then(Value::as_str)
                    .filter(|value| !value.is_empty())
            })
            .ok_or_else(|| AppleMusicApiError::Message("failed to get lyrics".into()))?;
        ttml_to_lrc(ttml)
    }

    async fn catalog<T: serde::de::DeserializeOwned>(
        &self,
        path: String,
        language: Option<&str>,
        dev_token: &str,
        music_token: Option<&str>,
        params: &[(&str, String)],
    ) -> ApiResult<T> {
        let url = if path.starts_with("http://") || path.starts_with("https://") {
            path
        } else {
            format!("https://amp-api.music.apple.com{path}")
        };

        let mut request = self
            .client
            .get(url)
            .header(AUTHORIZATION, format!("Bearer {dev_token}"))
            .header(ORIGIN, MUSIC_ORIGIN)
            .header(REFERER, format!("{MUSIC_ORIGIN}/"));
        if let Some(language) = language.filter(|value| !value.is_empty()) {
            request = request.query(&[("l", language)]);
        }
        if let Some(music_token) = music_token {
            request = request.header(COOKIE, format!("media-user-token={music_token}"));
        }
        if !params.is_empty() {
            request = request.query(params);
        }

        let response = request.send().await?;
        let status = response.status();
        if !status.is_success() {
            let retry_after = response
                .headers()
                .get(reqwest::header::RETRY_AFTER)
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned);
            return Err(AppleMusicApiError::UpstreamHttp {
                status,
                message: format!("apple api request failed: {status}"),
                retry_after,
            });
        }
        Ok(response.json().await?)
    }

    async fn catalog_json(
        &self,
        path: String,
        language: Option<&str>,
        dev_token: &str,
        music_token: Option<&str>,
        params: &[(&str, String)],
    ) -> ApiResult<Value> {
        self.catalog(path, language, dev_token, music_token, params)
            .await
    }

    async fn search_apple_catalog(&self, request: &SearchRequest<'_>) -> ApiResult<Value> {
        let _permit = self
            .search_gate
            .acquire()
            .await
            .expect("search semaphore should stay open");
        self.throttle_search().await;
        self.search_catalog_with_web_token(request, false).await
    }

    async fn search_catalog_with_web_token(
        &self,
        request: &SearchRequest<'_>,
        refresh_token: bool,
    ) -> ApiResult<Value> {
        let web_token = self.web_token(refresh_token).await?;
        let result = self
            .catalog_json(
                format!("/v1/catalog/{}/search", request.storefront),
                request.language,
                &web_token,
                None,
                &[
                    ("term", request.query.trim().to_owned()),
                    ("types", format!("{}s", request.search_type)),
                    ("limit", request.limit.to_string()),
                    ("offset", request.offset.to_string()),
                ],
            )
            .await;

        if !refresh_token
            && let Err(AppleMusicApiError::UpstreamHttp { status, .. }) = &result
            && matches!(
                *status,
                reqwest::StatusCode::UNAUTHORIZED | reqwest::StatusCode::FORBIDDEN
            )
        {
            let refreshed_web_token = self.web_token(true).await?;
            return self
                .catalog_json(
                    format!("/v1/catalog/{}/search", request.storefront),
                    request.language,
                    &refreshed_web_token,
                    None,
                    &[
                        ("term", request.query.trim().to_owned()),
                        ("types", format!("{}s", request.search_type)),
                        ("limit", request.limit.to_string()),
                        ("offset", request.offset.to_string()),
                    ],
                )
                .await;
        }

        result
    }

    async fn throttle_search(&self) {
        let mut next_allowed_at = self.search_next_allowed_at.lock().await;
        let now = Instant::now();
        if *next_allowed_at > now {
            tokio::time::sleep(*next_allowed_at - now).await;
        }
        *next_allowed_at = Instant::now() + SEARCH_THROTTLE_WINDOW;
    }

    async fn cached_search_payload(&self, key: &SearchCacheKey) -> Option<CachedSearchPayload> {
        let now = Instant::now();
        let mut search_cache = self.search_cache.lock().await;
        match search_cache.get(key) {
            Some(entry) if entry.expires_at > now => Some(entry.payload.clone()),
            Some(_) => {
                search_cache.remove(key);
                None
            }
            None => None,
        }
    }

    async fn insert_search_cache(
        &self,
        key: SearchCacheKey,
        payload: CachedSearchPayload,
        ttl: Duration,
    ) {
        self.search_cache.lock().await.insert(
            key,
            SearchCacheEntry {
                expires_at: Instant::now() + ttl,
                payload,
            },
        );
    }

    async fn finish_search_flight(&self, key: &SearchCacheKey) {
        let notify = self.search_inflight.lock().await.remove(key);
        if let Some(notify) = notify {
            notify.notify_waiters();
        }
    }

    async fn web_token(&self, refresh: bool) -> ApiResult<String> {
        if !refresh && let Some(cached) = self.cached_web_token().await {
            return Ok(cached);
        }

        let token = self.fetch_web_token().await?;
        let mut web_token = self.web_token.lock().await;
        *web_token = Some(WebTokenCacheEntry {
            token: token.clone(),
            expires_at: Instant::now() + WEB_TOKEN_TTL,
        });
        Ok(token)
    }

    async fn cached_web_token(&self) -> Option<String> {
        let web_token = self.web_token.lock().await;
        web_token
            .as_ref()
            .filter(|cached| cached.expires_at > Instant::now())
            .map(|cached| cached.token.clone())
    }

    async fn fetch_web_token(&self) -> ApiResult<String> {
        let homepage = self.client.get(MUSIC_ORIGIN).send().await?.text().await?;
        let index_js_path = extract_index_js_path(&homepage)?;
        let script = self
            .client
            .get(format!("{MUSIC_ORIGIN}{index_js_path}"))
            .send()
            .await?
            .text()
            .await?;
        extract_web_token(&script)
    }
}

fn retry_after_ttl(retry_after: Option<&str>) -> Duration {
    retry_after
        .and_then(|value| value.trim().parse::<u64>().ok())
        .map(Duration::from_secs)
        .filter(|value| !value.is_zero())
        .unwrap_or(SEARCH_RATE_LIMIT_TTL)
}

fn extract_index_js_path(homepage: &str) -> ApiResult<&str> {
    INDEX_JS_REGEX
        .find(homepage)
        .map(|capture| capture.as_str())
        .ok_or_else(|| {
            AppleMusicApiError::Protocol("music.apple.com homepage did not contain index js".into())
        })
}

fn extract_web_token(script: &str) -> ApiResult<String> {
    WEB_TOKEN_REGEX
        .find(script)
        .map(|capture| capture.as_str().to_owned())
        .ok_or_else(|| {
            AppleMusicApiError::Protocol("music.apple.com script did not contain web token".into())
        })
}

fn append_album_tracks(album: &mut Value, mut next_page: Value) -> ApiResult<()> {
    let extra_tracks = next_page
        .get_mut("data")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| {
            AppleMusicApiError::Protocol("album track page omitted data array".into())
        })?;
    let track_data = album
        .pointer_mut("/data/0/relationships/tracks/data")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| AppleMusicApiError::Protocol("album response omitted track data".into()))?;
    track_data.append(extra_tracks);

    let next_value = next_page.get("next").cloned().unwrap_or(Value::Null);
    let tracks = album
        .pointer_mut("/data/0/relationships/tracks")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| {
            AppleMusicApiError::Protocol("album response omitted track relationship".into())
        })?;
    tracks.insert("next".into(), next_value);
    Ok(())
}

pub fn ttml_to_lrc(ttml: &str) -> ApiResult<String> {
    let document = Document::parse(ttml)?;
    if ttml.contains("itunes:timing=\"None\"") {
        let lines = document
            .descendants()
            .filter(|node| node.is_element() && node.tag_name().name() == "p")
            .filter_map(|node| {
                let text = node_text(node);
                (!text.is_empty()).then_some(text)
            })
            .collect::<Vec<_>>();
        if lines.is_empty() {
            return Err(AppleMusicApiError::Message(
                "lyrics ttml did not contain any lines".into(),
            ));
        }
        return Ok(lines.join("\n"));
    }

    let mut lines = Vec::new();
    for node in document
        .descendants()
        .filter(|node| node.is_element() && node.tag_name().name() == "p")
    {
        let Some(begin) = node.attribute("begin") else {
            continue;
        };
        let text = node
            .attribute("text")
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| node_text(node));
        if text.is_empty() {
            continue;
        }
        let (minute, second, centisecond) = parse_ttml_timestamp(begin)?;
        lines.push(format!("[{minute:02}:{second:02}.{centisecond:02}]{text}"));
    }

    if lines.is_empty() {
        return Err(AppleMusicApiError::Message(
            "lyrics ttml did not contain any synchronized lines".into(),
        ));
    }
    Ok(lines.join("\n"))
}

fn node_text(node: roxmltree::Node<'_, '_>) -> String {
    node.descendants()
        .filter_map(|child| child.text())
        .collect::<Vec<_>>()
        .join("")
        .trim()
        .to_owned()
}

fn parse_ttml_timestamp(value: &str) -> ApiResult<(u32, u32, u32)> {
    let value = value.trim();
    let value = value.strip_suffix('s').unwrap_or(value);
    let parts = value.split(':').collect::<Vec<_>>();

    let (hours, minutes, seconds_part) = match parts.as_slice() {
        [hour, minute, second] => (
            hour.parse::<u32>().map_err(|error| {
                AppleMusicApiError::Protocol(format!("invalid TTML hour: {error}"))
            })?,
            minute.parse::<u32>().map_err(|error| {
                AppleMusicApiError::Protocol(format!("invalid TTML minute: {error}"))
            })?,
            *second,
        ),
        [minute, second] => (
            0,
            minute.parse::<u32>().map_err(|error| {
                AppleMusicApiError::Protocol(format!("invalid TTML minute: {error}"))
            })?,
            *second,
        ),
        [second] => (0, 0, *second),
        _ => {
            return Err(AppleMusicApiError::Protocol(format!(
                "unsupported TTML timestamp format: {value}"
            )));
        }
    };

    let seconds = seconds_part
        .parse::<f64>()
        .map_err(|error| AppleMusicApiError::Protocol(format!("invalid TTML second: {error}")))?;
    let total_centiseconds = ((hours * 3600 + minutes * 60) as f64 + seconds) * 100.0;
    let rounded = total_centiseconds.round() as u32;
    let total_seconds = rounded / 100;
    Ok((total_seconds / 60, total_seconds % 60, rounded % 100))
}

#[derive(Debug, Deserialize)]
struct SongPlaybackResponse {
    data: Vec<SongPlaybackData>,
}

#[derive(Debug, Deserialize)]
struct SongPlaybackData {
    attributes: SongPlaybackAttributes,
    relationships: SongPlaybackRelationships,
}

#[derive(Debug, Deserialize)]
struct SongPlaybackAttributes {
    #[serde(rename = "artistName")]
    artist_name: String,
    #[serde(rename = "albumName")]
    album_name: String,
    #[serde(rename = "trackNumber")]
    track_number: u32,
    #[serde(rename = "discNumber")]
    disc_number: u32,
    name: String,
    artwork: Option<Artwork>,
}

#[derive(Debug, Deserialize)]
struct SongPlaybackRelationships {
    artists: Relationship<ArtistData>,
    albums: Relationship<AlbumData>,
}

#[derive(Debug, Deserialize)]
struct Relationship<T> {
    data: Vec<T>,
}

#[derive(Debug, Deserialize)]
struct ArtistData {
    id: String,
}

#[derive(Debug, Deserialize)]
struct AlbumData {
    id: String,
    attributes: Option<AlbumAttributes>,
}

#[derive(Debug, Deserialize)]
struct AlbumAttributes {
    artwork: Option<Artwork>,
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{extract_index_js_path, extract_web_token, retry_after_ttl};

    #[test]
    fn extract_index_js_path_finds_music_web_bundle() {
        let homepage = r#"<script type="module" src="/assets/index~en-US.abcd1234.js"></script>"#;
        assert_eq!(
            extract_index_js_path(homepage).expect("index js path"),
            "/assets/index~en-US.abcd1234.js"
        );
    }

    #[test]
    fn extract_web_token_finds_jwt_like_value() {
        let script = r#"const token="eyJh.fake.web.token";"#;
        assert_eq!(
            extract_web_token(script).expect("web token"),
            "eyJh.fake.web.token"
        );
    }

    #[test]
    fn retry_after_ttl_uses_numeric_header_value() {
        assert_eq!(retry_after_ttl(Some("7")), Duration::from_secs(7));
    }

    #[test]
    fn retry_after_ttl_falls_back_for_invalid_value() {
        assert_eq!(
            retry_after_ttl(Some("Wed, 21 Oct 2015 07:28:00 GMT")),
            Duration::from_secs(2)
        );
        assert_eq!(retry_after_ttl(None), Duration::from_secs(2));
    }
}
