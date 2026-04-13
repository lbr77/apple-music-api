mod context;
mod http;
mod response;
mod subsonic;

use std::sync::Arc;

use apple_music_decryptor::NativePlatform;
use axum::Router;

use crate::config::AppConfig;
use crate::error::{AppError, AppResult};
use crate::runtime::AppState;

pub(crate) use context::DaemonContext;

pub async fn run_daemon_server(
    config: AppConfig,
    platform: Arc<NativePlatform>,
    state: Arc<AppState>,
) -> AppResult<()> {
    std::fs::create_dir_all(config.cache_dir.join("lyrics"))?;
    std::fs::create_dir_all(config.cache_dir.join("albums"))?;

    let context = Arc::new(DaemonContext::new(config.clone(), platform, state)?);
    let app = Router::new()
        .merge(http::legacy_routes(Arc::clone(&context)))
        // Subsonic clients authenticate with Subsonic query credentials, so these routes stay on
        // a separate router and keep the Bearer middleware out of the /rest surface.
        .merge(subsonic::router(Arc::clone(&context)))
        .with_state(context);

    let listener = tokio::net::TcpListener::bind(config.daemon_addr()).await?;
    crate::app_info!(
        "daemon",
        "listening for daemon http requests on {}",
        config.daemon_addr(),
    );
    axum::serve(listener, app)
        .await
        .map_err(|error| AppError::Message(format!("daemon server failed: {error}")))?;
    Ok(())
}
