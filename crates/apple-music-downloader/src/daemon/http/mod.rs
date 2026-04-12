mod account;
mod auth;
mod catalog;

use std::sync::Arc;

use axum::extract::Request;
use axum::response::Response;
use axum::routing::{get, post};
use axum::{Router, middleware};
use tower_http::services::ServeDir;

use super::context::DaemonContext;

pub(super) fn legacy_routes(context: Arc<DaemonContext>) -> Router<Arc<DaemonContext>> {
    Router::new()
        .route("/health", get(account::health_handler))
        .route("/status", get(account::status_handler))
        .route("/login", post(account::login_handler))
        .route("/login/2fa", post(account::submit_two_factor_handler))
        .route("/login/reset", post(account::reset_login_handler))
        .route("/logout", post(account::logout_handler))
        .route("/search", get(catalog::search_handler))
        .route("/artist/{id}", get(catalog::artist_handler))
        .route(
            "/artist/{id}/view/{name}",
            get(catalog::artist_view_handler),
        )
        .route("/album/{id}", get(catalog::album_handler))
        .route("/song/{id}", get(catalog::song_handler))
        .route("/lyrics/{id}", get(catalog::lyrics_handler))
        .route("/playback/{id}", get(catalog::playback_handler))
        .nest_service("/cache", ServeDir::new(context.config.cache_dir.clone()))
        .layer(middleware::from_fn(log_http_request))
        .layer(middleware::from_fn_with_state(
            context,
            auth::require_bearer_auth,
        ))
}

async fn log_http_request(request: Request, next: middleware::Next) -> Response {
    let method = request.method().clone();
    let path = request.uri().path().to_owned();
    let query = request.uri().query().unwrap_or("").to_owned();
    crate::app_debug!(
        "http::request",
        "incoming request: method={}, path={}, query={}",
        method,
        path,
        query,
    );
    let response = next.run(request).await;
    crate::app_info!("http::request", "{} {} {}", method, path, response.status(),);
    response
}

fn log_http_completion(target: &str, action: &str, result: Result<&str, &str>) {
    match result {
        Ok(state) => crate::app_debug!(target, "{action} completed: state={state}"),
        Err(message) => crate::app_debug!(target, "{action} failed: {message}"),
    }
}
