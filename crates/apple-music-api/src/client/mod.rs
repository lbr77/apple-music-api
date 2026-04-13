mod lyrics;
mod search;
mod web_token;

use std::sync::Arc;
use std::time::Instant;

use reqwest::header::{AUTHORIZATION, COOKIE, ORIGIN, REFERER};
use reqwest::{Client, Proxy, Request, RequestBuilder};
use serde::Deserialize;
use serde_json::Value;
use tokio::sync::{Mutex, Notify, Semaphore};

use crate::error::{ApiResult, AppleMusicApiError};

use self::search::{SearchCacheEntry, SearchCacheKey};
use self::web_token::WebTokenCacheEntry;

const MUSIC_ORIGIN: &str = "https://music.apple.com";

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

#[derive(Clone)]
pub struct AppleApiClient {
    client: Client,
    search_cache: Arc<Mutex<std::collections::HashMap<SearchCacheKey, SearchCacheEntry>>>,
    search_inflight: Arc<Mutex<std::collections::HashMap<SearchCacheKey, Arc<Notify>>>>,
    search_gate: Arc<Semaphore>,
    search_next_allowed_at: Arc<Mutex<Instant>>,
    web_token: Arc<Mutex<Option<WebTokenCacheEntry>>>,
}

impl AppleApiClient {
    pub fn new(proxy: Option<&str>) -> ApiResult<Self> {
        let mut builder = Client::builder().user_agent(
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
             (KHTML, like Gecko) Chrome/91.0.4472.124 Safari/537.36",
        );
        if let Some(proxy) = proxy {
            crate::app_info!(
                "http::apple_api",
                "configuring upstream proxy for Apple API client"
            );
            builder = builder.proxy(Proxy::all(proxy)?);
        }
        Ok(Self {
            client: builder.build()?,
            search_cache: Arc::new(Mutex::new(std::collections::HashMap::new())),
            search_inflight: Arc::new(Mutex::new(std::collections::HashMap::new())),
            search_gate: Arc::new(Semaphore::new(search::SEARCH_MAX_CONCURRENCY)),
            search_next_allowed_at: Arc::new(Mutex::new(Instant::now())),
            web_token: Arc::new(Mutex::new(None)),
        })
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
        let mut params = vec![("include", "genres,station".to_owned())];
        params.push((
            "views",
            views
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or(
                    "top-songs,latest-release,full-albums,singles,featured-playlists,playlists,similar-artists,top-music-videos",
                )
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

        self.send_json(request).await
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

    async fn lyrics_catalog_json(
        &self,
        url: &str,
        dev_token: &str,
        music_token: &str,
    ) -> ApiResult<Value> {
        let request = self
            .client
            .get(url)
            .header(AUTHORIZATION, format!("Bearer {dev_token}"))
            .header(ORIGIN, MUSIC_ORIGIN)
            .header(REFERER, format!("{MUSIC_ORIGIN}/"))
            .header("media-user-token", music_token)
            .header(COOKIE, format!("media-user-token={music_token}"));
        self.send_json(request).await
    }

    async fn send_json<T: serde::de::DeserializeOwned>(
        &self,
        request: RequestBuilder,
    ) -> ApiResult<T> {
        let request = request.build()?;
        log_request(&request);
        let response = self.client.execute(request).await?;
        let status = response.status();
        if !status.is_success() {
            let retry_after = response
                .headers()
                .get(reqwest::header::RETRY_AFTER)
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned);
            crate::app_warn!(
                "http::apple_api",
                "Apple API request failed: status={}, retry_after={}",
                status,
                retry_after.as_deref().unwrap_or("-"),
            );
            return Err(AppleMusicApiError::UpstreamHttp {
                status,
                message: format!("apple api request failed: {status}"),
                retry_after,
            });
        }
        crate::app_info!(
            "http::apple_api",
            "Apple API request completed: status={status}"
        );
        Ok(response.json().await?)
    }
}

fn log_request(request: &Request) {
    crate::app_info!(
        "http::apple_api",
        "sending Apple API request: method={}, path={}, query={}",
        request.method(),
        request.url().path(),
        request.url().query().unwrap_or(""),
    );
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
