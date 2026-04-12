use std::sync::Arc;

use axum::extract::{Request, State};
use axum::http::header::AUTHORIZATION;
use axum::middleware::Next;
use axum::response::Response;

use crate::daemon::context::DaemonContext;
use crate::daemon::response::ApiError;

pub(super) async fn require_bearer_auth(
    State(context): State<Arc<DaemonContext>>,
    request: Request,
    next: Next,
) -> Result<Response, ApiError> {
    let path = request.uri().path().to_owned();
    let Some(header) = request.headers().get(AUTHORIZATION) else {
        crate::app_debug!(
            "http::auth",
            "rejecting request without bearer token: path={path}"
        );
        return Err(ApiError::unauthorized(&path, "missing bearer token"));
    };
    let Ok(header) = header.to_str() else {
        crate::app_debug!(
            "http::auth",
            "rejecting request with invalid header encoding: path={path}",
        );
        return Err(ApiError::unauthorized(
            &path,
            "invalid authorization header encoding",
        ));
    };
    let Some(token) = header.strip_prefix("Bearer ") else {
        crate::app_debug!(
            "http::auth",
            "rejecting request with non-bearer authorization scheme: path={path}",
        );
        return Err(ApiError::unauthorized(
            &path,
            "authorization header must use Bearer",
        ));
    };
    if token != context.api_token() {
        crate::app_debug!(
            "http::auth",
            "rejecting request with invalid token: path={path}"
        );
        return Err(ApiError::unauthorized(&path, "invalid bearer token"));
    }

    crate::app_debug!("http::auth", "accepted authenticated request: path={path}");
    Ok(next.run(request).await)
}
