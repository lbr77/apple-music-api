pub mod config;
pub mod daemon;
pub mod error;
pub mod ffi;
pub mod launcher;
pub mod logging;
pub mod runtime;

use std::sync::Arc;

use config::AppConfig;
use daemon::run_daemon_server;
use error::AppResult;
use ffi::{NativePlatform, install_android_log_shim};
use runtime::AppState;

pub async fn run_server_process() -> AppResult<()> {
    install_android_log_shim();
    let config = AppConfig::parse()?;
    crate::app_info!(
        "main",
        "parsed config: daemon_addr={}, decrypt_workers={}, decrypt_inflight={}, base_dir={}, library_dir={}, cache_dir={}, storefront={}, language={}",
        config.daemon_addr(),
        config.decrypt_workers,
        config.decrypt_inflight,
        config.base_dir.display(),
        config.library_dir.display(),
        config.cache_dir.display(),
        config.storefront,
        config.language,
    );

    crate::app_info!("main", "bootstrapping native platform");
    let platform = Arc::new(NativePlatform::bootstrap(config.clone())?);
    let state = Arc::new(AppState::default());
    crate::app_info!("main", "native platform bootstrap completed");

    crate::app_info!("main", "starting daemon http server");
    tokio::spawn(run_daemon_server(config, platform, state))
        .await
        .map_err(|error| {
            crate::error::AppError::Message(format!("daemon server panicked: {error}"))
        })??;
    Ok(())
}
