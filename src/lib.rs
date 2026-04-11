pub mod config;
pub mod daemon;
pub mod error;
pub mod ffi;
pub mod logging;
pub mod runtime;

use std::sync::Arc;

use config::AppConfig;
use daemon::run_daemon_server;
use error::AppResult;
use ffi::{NativePlatform, install_android_log_shim};
use runtime::{AppState, SessionRuntime};

pub async fn run_server_process() -> AppResult<()> {
    install_android_log_shim();
    let config = AppConfig::parse()?;
    run_server(config).await
}

pub fn run_server_process_blocking(config: AppConfig) -> AppResult<()> {
    install_android_log_shim();
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(run_server(config))
}

async fn run_server(config: AppConfig) -> AppResult<()> {
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
    restore_startup_session(&platform, &state);

    crate::app_info!("main", "starting daemon http server");
    tokio::spawn(run_daemon_server(config, platform, state))
        .await
        .map_err(|error| {
            crate::error::AppError::Message(format!("daemon server panicked: {error}"))
        })??;
    Ok(())
}

fn restore_startup_session(platform: &NativePlatform, state: &AppState) {
    match platform.restore_session() {
        Ok(Some(session)) => match SessionRuntime::new(session) {
            Ok(session) => {
                state.replace_session(Arc::new(session));
                crate::app_info!("main", "restored persisted login state during startup");
            }
            Err(error) => {
                crate::app_warn!(
                    "main",
                    "startup session restore built a native session but profile recovery failed: {error}"
                );
            }
        },
        Ok(None) => {}
        Err(error) => {
            crate::app_warn!("main", "startup session restore failed: {error}");
        }
    }
}
