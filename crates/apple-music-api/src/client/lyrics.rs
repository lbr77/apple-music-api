use roxmltree::Document;
use serde_json::Value;

use crate::error::{ApiResult, AppleMusicApiError};

use super::AppleApiClient;

impl AppleApiClient {
    pub async fn lyrics(
        &self,
        storefront: &str,
        language: Option<&str>,
        music_token: &str,
        song_id: &str,
    ) -> ApiResult<String> {
        let mut last_error = None;
        for endpoint in lyrics_endpoints(language) {
            match self
                .lyrics_response(storefront, language, music_token, song_id, endpoint)
                .await
            {
                Ok(response) => {
                    if let Some(ttml) = lyrics_ttml(&response) {
                        return ttml_to_lrc(ttml);
                    }
                }
                Err(AppleMusicApiError::UpstreamHttp { status, .. }) if status.as_u16() == 404 => {
                    last_error = Some(AppleMusicApiError::UpstreamHttp {
                        status,
                        message: format!("apple api request failed: {status}"),
                        retry_after: None,
                    });
                }
                Err(error) => return Err(error),
            }
        }

        Err(last_error
            .unwrap_or_else(|| AppleMusicApiError::Message("failed to get lyrics".into())))
    }

    async fn lyrics_response(
        &self,
        storefront: &str,
        language: Option<&str>,
        music_token: &str,
        song_id: &str,
        endpoint: &str,
    ) -> ApiResult<Value> {
        let web_token = self.web_token(false).await?;
        let path = format!("/v1/catalog/{storefront}/songs/{song_id}/{endpoint}");
        let url = lyrics_request_url(&path, language);
        let result = self
            .lyrics_catalog_json(url.as_str(), &web_token, music_token)
            .await;
        if let Err(AppleMusicApiError::UpstreamHttp { status, .. }) = &result
            && matches!(
                *status,
                reqwest::StatusCode::UNAUTHORIZED | reqwest::StatusCode::FORBIDDEN
            )
        {
            let refreshed_web_token = self.web_token(true).await?;
            return self
                .lyrics_catalog_json(url.as_str(), &refreshed_web_token, music_token)
                .await;
        }
        result
    }
}

fn lyrics_request_url(path: &str, language: Option<&str>) -> String {
    let mut params = Vec::new();
    if let Some(language) = language.map(str::trim).filter(|value| !value.is_empty()) {
        let language = language.trim_start_matches('?');
        if language.contains('=') || language.contains('&') {
            params.push(language.to_owned());
        } else {
            params.push(format!("l={language}"));
        }
    }
    if !params.iter().any(|value| value.contains("extend=")) {
        params.push("extend=ttmlLocalizations".into());
    }

    format!("https://amp-api.music.apple.com{path}?{}", params.join("&"))
}

fn lyrics_endpoints(language: Option<&str>) -> [&'static str; 2] {
    if prefers_syllable_lyrics(language) {
        ["syllable-lyrics", "lyrics"]
    } else {
        ["lyrics", "syllable-lyrics"]
    }
}

fn prefers_syllable_lyrics(language: Option<&str>) -> bool {
    let Some(language) = language.map(str::trim).filter(|value| !value.is_empty()) else {
        return false;
    };

    let normalized = language.trim_start_matches('?').to_ascii_lowercase();
    normalized.contains("l[lyrics]") || normalized.contains("l%5blyrics%5d")
}

fn lyrics_ttml(response: &Value) -> Option<&str> {
    response
        .pointer("/data/0/attributes/ttml")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .or_else(|| {
            response
                .pointer("/data/0/attributes/ttmlLocalizations")
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
        })
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
    render_node_text(node).trim().to_owned()
}

fn render_node_text(node: roxmltree::Node<'_, '_>) -> String {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum PieceKind {
        DirectText,
        LeafElement,
        NestedElement,
    }

    struct Piece {
        kind: PieceKind,
        text: String,
    }

    let mut pieces = Vec::new();
    for child in node.children() {
        if child.is_text() {
            if let Some(text) = child
                .text()
                .filter(|value| value.chars().any(|ch| !ch.is_whitespace()))
            {
                pieces.push(Piece {
                    kind: PieceKind::DirectText,
                    text: text.to_owned(),
                });
            }
            continue;
        }

        if !child.is_element() {
            continue;
        }

        let text = render_node_text(child);
        if text.trim().is_empty() {
            continue;
        }

        let kind = if child.children().any(|grandchild| grandchild.is_element()) {
            PieceKind::NestedElement
        } else {
            PieceKind::LeafElement
        };
        pieces.push(Piece { kind, text });
    }

    if pieces.is_empty() {
        return String::new();
    }

    // Apple syllable TTML can expose the same rendered line through parallel nested branches.
    // Concatenating every descendant text doubles the line before it is written into the MP4 tag.
    if pieces
        .iter()
        .all(|piece| piece.kind == PieceKind::NestedElement)
    {
        let candidate = pieces[0].text.trim();
        if !candidate.is_empty() && pieces.iter().all(|piece| piece.text.trim() == candidate) {
            return candidate.to_owned();
        }
    }

    let direct_text = pieces
        .iter()
        .filter(|piece| piece.kind == PieceKind::DirectText)
        .map(|piece| piece.text.as_str())
        .collect::<String>();
    let has_nested_elements = pieces
        .iter()
        .any(|piece| piece.kind == PieceKind::NestedElement);
    if has_nested_elements
        && !direct_text.trim().is_empty()
        && pieces
            .iter()
            .filter(|piece| piece.kind == PieceKind::NestedElement)
            .map(|piece| piece.text.trim())
            .all(|piece| piece == direct_text.trim())
        && pieces
            .iter()
            .all(|piece| piece.kind != PieceKind::LeafElement)
    {
        return direct_text.trim().to_owned();
    }

    pieces.into_iter().map(|piece| piece.text).collect()
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

#[cfg(test)]
mod tests {
    use std::env;

    use super::{AppleApiClient, lyrics_endpoints, lyrics_request_url, ttml_to_lrc};

    #[test]
    fn lyrics_request_url_preserves_structured_language_params() {
        let url = lyrics_request_url(
            "/v1/catalog/cn/songs/1648869428/syllable-lyrics",
            Some("l[lyrics]=zh-hans-cn&l[script]=zh-Hans"),
        );
        assert_eq!(
            url,
            "https://amp-api.music.apple.com/v1/catalog/cn/songs/1648869428/syllable-lyrics?l[lyrics]=zh-hans-cn&l[script]=zh-Hans&extend=ttmlLocalizations"
        );
    }

    #[test]
    fn lyrics_endpoints_prefers_syllable_when_language_requests_lyrics_locale() {
        assert_eq!(
            lyrics_endpoints(Some("l[lyrics]=zh-hans-cn&l[script]=zh-Hans")),
            ["syllable-lyrics", "lyrics"]
        );
        assert_eq!(
            lyrics_endpoints(Some("l%5Blyrics%5D=zh-hans-cn&l%5Bscript%5D=zh-Hans")),
            ["syllable-lyrics", "lyrics"]
        );
    }

    #[test]
    fn lyrics_endpoints_default_to_standard_lyrics() {
        assert_eq!(lyrics_endpoints(None), ["lyrics", "syllable-lyrics"]);
        assert_eq!(lyrics_endpoints(Some("ja")), ["lyrics", "syllable-lyrics"]);
    }

    #[test]
    fn ttml_to_lrc_concatenates_normal_syllable_spans() {
        let lyrics = ttml_to_lrc(
            r#"
            <tt xmlns="http://www.w3.org/ns/ttml">
              <body>
                <div>
                  <p begin="00:20.02">
                    <span>摇曳海中</span><span>扁舟 </span><span>或穿越丛林的风</span>
                  </p>
                </div>
              </body>
            </tt>
            "#,
        )
        .expect("ttml should parse");

        assert_eq!(lyrics, "[00:20.02]摇曳海中扁舟 或穿越丛林的风");
    }

    #[test]
    fn ttml_to_lrc_ignores_duplicate_nested_branches() {
        let lyrics = ttml_to_lrc(
            r#"
            <tt xmlns="http://www.w3.org/ns/ttml">
              <body>
                <div>
                  <p begin="00:20.02">
                    <span>
                      <span>摇曳海中</span><span>扁舟 </span><span>或穿越丛林的风</span>
                    </span>
                    <span>
                      <span>摇曳海中</span><span>扁舟 </span><span>或穿越丛林的风</span>
                    </span>
                  </p>
                </div>
              </body>
            </tt>
            "#,
        )
        .expect("ttml should parse");

        assert_eq!(lyrics, "[00:20.02]摇曳海中扁舟 或穿越丛林的风");
    }

    #[tokio::test]
    #[ignore = "requires live Apple Music credentials in env"]
    async fn live_lyrics_fetch_uses_env_credentials() {
        let storefront = required_env("APPLE_MUSIC_TEST_STOREFRONT");
        let song_id = required_env("APPLE_MUSIC_TEST_SONG_ID");
        let music_token = required_env("APPLE_MUSIC_TEST_MEDIA_USER_TOKEN");
        let language = optional_env("APPLE_MUSIC_TEST_LANGUAGE");
        let proxy = optional_env("APPLE_MUSIC_TEST_PROXY");

        let client = AppleApiClient::new(proxy.as_deref()).expect("client should initialize");
        let lyrics = client
            .lyrics(
                storefront.as_str(),
                language.as_deref(),
                music_token.as_str(),
                song_id.as_str(),
            )
            .await
            .expect("lyrics request should succeed");

        assert!(
            lyrics.lines().next().is_some(),
            "lyrics should contain at least one line"
        );
    }

    fn required_env(name: &str) -> String {
        env::var(name).unwrap_or_else(|_| panic!("{name} must be set"))
    }

    fn optional_env(name: &str) -> Option<String> {
        env::var(name)
            .ok()
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
    }
}
