use reqwest::header::{AUTHORIZATION, COOKIE, ORIGIN, REFERER};
use reqwest::{Client, Proxy};
use roxmltree::Document;
use serde_json::Value;

use crate::config::AppConfig;
use crate::error::{AppError, AppResult};

const MUSIC_ORIGIN: &str = "https://music.apple.com";
const DESKTOP_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/91.0.4472.124 Safari/537.36";

#[derive(Clone)]
pub struct AppleApiClient {
    client: Client,
}

impl AppleApiClient {
    pub fn new(config: &AppConfig) -> AppResult<Self> {
        let mut builder = Client::builder().user_agent(DESKTOP_USER_AGENT);
        if let Some(proxy) = config.proxy.as_deref() {
            builder = builder.proxy(Proxy::all(proxy)?);
        }
        Ok(Self {
            client: builder.build()?,
        })
    }

    pub async fn search(
        &self,
        storefront: &str,
        language: Option<&str>,
        dev_token: &str,
        query: &str,
        search_type: &str,
        limit: usize,
        offset: usize,
    ) -> AppResult<Value> {
        self.catalog_json(
            format!("/v1/catalog/{storefront}/search"),
            language,
            dev_token,
            None,
            &[
                ("term", query.to_owned()),
                ("types", format!("{search_type}s")),
                ("limit", limit.to_string()),
                ("offset", offset.to_string()),
            ],
        )
        .await
    }

    pub async fn album(
        &self,
        storefront: &str,
        language: Option<&str>,
        dev_token: &str,
        album_id: &str,
    ) -> AppResult<Value> {
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

        loop {
            let Some(next_path) = album
                .pointer("/data/0/relationships/tracks/next")
                .and_then(Value::as_str)
                .map(str::to_owned)
            else {
                break;
            };

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
    ) -> AppResult<Value> {
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

    pub async fn lyrics(
        &self,
        storefront: &str,
        language: Option<&str>,
        dev_token: &str,
        music_token: &str,
        song_id: &str,
    ) -> AppResult<String> {
        let response = self
            .catalog_json(
                format!("/v1/catalog/{storefront}/songs/{song_id}/lyrics"),
                language,
                dev_token,
                Some(music_token),
                &[("extend", "ttmlLocalizations".into())],
            )
            .await?;
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
            .ok_or_else(|| AppError::Message("failed to get lyrics".into()))?;
        ttml_to_lrc(ttml)
    }

    async fn catalog_json(
        &self,
        path: String,
        language: Option<&str>,
        dev_token: &str,
        music_token: Option<&str>,
        params: &[(&str, String)],
    ) -> AppResult<Value> {
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
            return Err(AppError::Message(format!(
                "apple api request failed: {status}"
            )));
        }
        Ok(response.json().await?)
    }
}

fn append_album_tracks(album: &mut Value, mut next_page: Value) -> AppResult<()> {
    let extra_tracks = next_page
        .get_mut("data")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| AppError::Protocol("album track page omitted data array".into()))?;
    let track_data = album
        .pointer_mut("/data/0/relationships/tracks/data")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| AppError::Protocol("album response omitted track data".into()))?;
    track_data.append(extra_tracks);

    let next_value = next_page.get("next").cloned().unwrap_or(Value::Null);
    let tracks = album
        .pointer_mut("/data/0/relationships/tracks")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| AppError::Protocol("album response omitted track relationship".into()))?;
    tracks.insert("next".into(), next_value);
    Ok(())
}

pub fn ttml_to_lrc(ttml: &str) -> AppResult<String> {
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
            return Err(AppError::Message(
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
        return Err(AppError::Message(
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

fn parse_ttml_timestamp(value: &str) -> AppResult<(u32, u32, u32)> {
    let value = value.trim();
    let value = value.strip_suffix('s').unwrap_or(value);
    let parts = value.split(':').collect::<Vec<_>>();

    let (hours, minutes, seconds_part) = match parts.as_slice() {
        [hour, minute, second] => (
            hour.parse::<u32>()
                .map_err(|error| AppError::Protocol(format!("invalid TTML hour: {error}")))?,
            minute
                .parse::<u32>()
                .map_err(|error| AppError::Protocol(format!("invalid TTML minute: {error}")))?,
            *second,
        ),
        [minute, second] => (
            0,
            minute
                .parse::<u32>()
                .map_err(|error| AppError::Protocol(format!("invalid TTML minute: {error}")))?,
            *second,
        ),
        [second] => (0, 0, *second),
        _ => {
            return Err(AppError::Protocol(format!(
                "unsupported TTML timestamp format: {value}"
            )));
        }
    };

    let seconds = seconds_part
        .parse::<f64>()
        .map_err(|error| AppError::Protocol(format!("invalid TTML second: {error}")))?;
    let total_centiseconds = ((hours * 3600 + minutes * 60) as f64 + seconds) * 100.0;
    let rounded = total_centiseconds.round() as u32;
    let total_seconds = rounded / 100;
    Ok((total_seconds / 60, total_seconds % 60, rounded % 100))
}
