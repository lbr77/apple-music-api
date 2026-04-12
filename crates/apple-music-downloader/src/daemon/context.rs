use std::sync::Arc;

use apple_music_api::AppleApiClient;
use apple_music_decryptor::{NativePlatform, SessionRuntime};

use crate::config::AppConfig;
use crate::error::{AppError, AppResult};
use crate::runtime::AppState;

#[derive(Clone)]
pub(crate) struct DaemonContext {
    pub(super) config: AppConfig,
    pub(super) platform: Arc<NativePlatform>,
    pub(super) state: Arc<AppState>,
    pub(super) api: AppleApiClient,
}

impl DaemonContext {
    pub(super) fn new(
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

    pub(super) fn session(&self) -> AppResult<Arc<SessionRuntime>> {
        self.state.session().ok_or(AppError::NoActiveSession)
    }

    pub(super) fn default_storefront(&self) -> &str {
        &self.config.storefront
    }

    pub(super) fn default_language(&self) -> Option<&str> {
        (!self.config.language.is_empty()).then_some(self.config.language.as_str())
    }

    pub(super) fn api_token(&self) -> &str {
        &self.config.api_token
    }

    pub(super) fn subsonic_username(&self) -> &str {
        &self.config.subsonic_username
    }

    pub(super) fn subsonic_password(&self) -> &str {
        &self.config.subsonic_password
    }
}

pub(super) fn resolve_storefront(
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

pub(super) fn resolve_language<'a>(
    requested: Option<&'a str>,
    configured: Option<&'a str>,
) -> Option<&'a str> {
    requested
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .or(configured)
}
