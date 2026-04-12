use std::sync::Arc;

use crate::error::AppResult;
use crate::ffi::{AccountProfile, NativeSession};

pub struct SessionRuntime {
    native: Arc<NativeSession>,
    account_profile: Arc<AccountProfile>,
}

impl SessionRuntime {
    pub fn new(native: NativeSession) -> AppResult<Self> {
        crate::app_info!("runtime::session", "creating session runtime");
        let native = Arc::new(native);
        let account_profile = Arc::new(native.load_account_profile()?);
        Ok(Self {
            native,
            account_profile,
        })
    }

    pub fn native(&self) -> Arc<NativeSession> {
        Arc::clone(&self.native)
    }

    pub fn account_profile(&self) -> Arc<AccountProfile> {
        Arc::clone(&self.account_profile)
    }

    #[allow(dead_code)]
    pub fn refresh_lease(&self) -> AppResult<()> {
        crate::app_info!("runtime::session", "refresh_lease requested");
        self.native.refresh_lease()?;
        crate::app_info!("runtime::session", "refresh_lease completed");
        Ok(())
    }

    pub fn resolve_m3u8_url(&self, adam: u64) -> AppResult<String> {
        self.native
            .resolve_m3u8_url(adam, self.account_profile.offline_available)
    }
}
