use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::error::{ApiResult, AppleMusicApiError};

use super::{AppleApiClient, SearchRequest};

pub(super) const SEARCH_CACHE_TTL: Duration = Duration::from_secs(15);
const SEARCH_THROTTLE_WINDOW: Duration = Duration::from_millis(400);
const SEARCH_RATE_LIMIT_TTL: Duration = Duration::from_secs(2);
pub(super) const SEARCH_MAX_CONCURRENCY: usize = 1;

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
pub(super) struct SearchCacheKey {
    storefront: String,
    language: Option<String>,
    query: String,
    search_type: String,
    limit: usize,
    offset: usize,
}

#[derive(Clone)]
pub(super) struct SearchCacheEntry {
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

impl CachedUpstreamError {
    fn into_app_error(self) -> AppleMusicApiError {
        AppleMusicApiError::UpstreamHttp {
            status: self.status,
            message: self.message,
            retry_after: self.retry_after,
        }
    }
}

impl AppleApiClient {
    pub async fn search(&self, request: SearchRequest<'_>) -> ApiResult<Value> {
        let key = request.cache_key();

        loop {
            if let Some(cached) = self.cached_search_payload(&key).await {
                crate::app_info!(
                    "http::apple_api",
                    "serving cached search result: storefront={}, type={}, limit={}, offset={}, query_len={}",
                    request.storefront,
                    request.search_type,
                    request.limit,
                    request.offset,
                    request.query.trim().len(),
                );
                return cached.into_result();
            }

            let inflight = {
                let mut search_inflight = self.search_inflight.lock().await;
                if let Some(notify) = search_inflight.get(&key) {
                    Some(Arc::clone(notify))
                } else {
                    let notify = Arc::new(tokio::sync::Notify::new());
                    search_inflight.insert(key.clone(), Arc::clone(&notify));
                    None
                }
            };

            if let Some(notify) = inflight {
                crate::app_info!(
                    "http::apple_api",
                    "waiting for in-flight search result: storefront={}, type={}, query_len={}",
                    request.storefront,
                    request.search_type,
                    request.query.trim().len(),
                );
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
}

fn retry_after_ttl(retry_after: Option<&str>) -> Duration {
    retry_after
        .and_then(|value| value.trim().parse::<u64>().ok())
        .map(Duration::from_secs)
        .filter(|value| !value.is_zero())
        .unwrap_or(SEARCH_RATE_LIMIT_TTL)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::retry_after_ttl;

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
