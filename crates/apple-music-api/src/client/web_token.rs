use std::sync::LazyLock;
use std::time::{Duration, Instant};

use regex::Regex;

use crate::error::{ApiResult, AppleMusicApiError};

use super::{AppleApiClient, MUSIC_ORIGIN};

const WEB_TOKEN_TTL: Duration = Duration::from_secs(30 * 60);

static INDEX_JS_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"/assets/index~[^/"']+\.js"#).expect("index js regex should compile")
});
static WEB_TOKEN_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"eyJh[^"']*"#).expect("web token regex should compile"));

pub(super) struct WebTokenCacheEntry {
    pub(super) token: String,
    pub(super) expires_at: Instant,
}

impl AppleApiClient {
    pub(super) async fn web_token(&self, refresh: bool) -> ApiResult<String> {
        if !refresh && let Some(cached) = self.cached_web_token().await {
            crate::app_info!("account::apple_api", "reusing cached Apple web token");
            return Ok(cached);
        }

        if refresh {
            crate::app_info!(
                "account::apple_api",
                "refreshing Apple web token after auth failure"
            );
        } else {
            crate::app_info!("account::apple_api", "fetching Apple web token");
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
        crate::app_info!(
            "account::apple_api",
            "requesting Apple Music homepage for web token"
        );
        let homepage = self.client.get(MUSIC_ORIGIN).send().await?.text().await?;
        let index_js_path = extract_index_js_path(&homepage)?;
        crate::app_info!(
            "account::apple_api",
            "requesting Apple Music bootstrap script: path={index_js_path}",
        );
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

#[cfg(test)]
mod tests {
    use super::{extract_index_js_path, extract_web_token};

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
}
