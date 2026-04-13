use axum::extract::Path;
use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE};
use axum::http::{HeaderValue, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::get;
use axum::{Router, body::Body};
use include_dir::{Dir, File, include_dir};
use mime_guess::from_path;

static FRONTEND_DIST: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/../../frontend/dist");
const FRONTEND_INDEX_PATH: &str = "index.html";

pub(super) fn router<S>() -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    Router::new()
        // The UI stays public so the browser can collect the Bearer token before calling the API.
        .route("/", get(root_redirect_handler))
        .route("/app", get(index_handler))
        .route("/app/", get(index_handler))
        .route("/app/{*path}", get(asset_handler))
}

async fn root_redirect_handler() -> Redirect {
    Redirect::temporary("/app/")
}

async fn index_handler() -> Response {
    index_response()
}

async fn asset_handler(Path(path): Path<String>) -> Response {
    let normalized = path.trim_start_matches('/');

    if normalized.is_empty() {
        return index_response();
    }

    if let Some(file) = FRONTEND_DIST.get_file(normalized) {
        return file_response(normalized, file);
    }

    if normalized.contains('.') {
        return StatusCode::NOT_FOUND.into_response();
    }

    index_response()
}

fn index_response() -> Response {
    FRONTEND_DIST
        .get_file(FRONTEND_INDEX_PATH)
        .map(|file| file_response(FRONTEND_INDEX_PATH, file))
        .unwrap_or_else(|| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "frontend bundle missing: build frontend/dist before compiling the daemon",
            )
                .into_response()
        })
}

fn file_response(path: &str, file: &File<'_>) -> Response {
    let cache_control = if path.starts_with("assets/") {
        HeaderValue::from_static("public, max-age=31536000, immutable")
    } else {
        HeaderValue::from_static("no-cache")
    };

    let mime = from_path(path).first_or_octet_stream();
    let mut response = (StatusCode::OK, Body::from(file.contents().to_vec())).into_response();
    response.headers_mut().insert(
        CONTENT_TYPE,
        HeaderValue::from_str(mime.as_ref()).expect("valid mime header"),
    );
    response.headers_mut().insert(CACHE_CONTROL, cache_control);
    response
}

#[cfg(test)]
mod tests {
    use super::router;
    use axum::body::to_bytes;
    use axum::http::StatusCode;
    use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE, LOCATION};
    use axum::http::{Request, Uri};
    use tower::ServiceExt;

    #[tokio::test]
    async fn root_redirects_to_app() {
        let response = router()
            .oneshot(
                Request::builder()
                    .uri(Uri::from_static("/"))
                    .body(axum::body::Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::TEMPORARY_REDIRECT);
        assert_eq!(
            response
                .headers()
                .get(LOCATION)
                .expect("location header")
                .to_str()
                .expect("utf-8"),
            "/app/"
        );
    }

    #[tokio::test]
    async fn app_index_is_served_without_auth() {
        let response = router()
            .oneshot(
                Request::builder()
                    .uri(Uri::from_static("/app/"))
                    .body(axum::body::Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get(CONTENT_TYPE)
                .expect("content-type")
                .to_str()
                .expect("utf-8"),
            "text/html"
        );
        assert_eq!(
            response
                .headers()
                .get(CACHE_CONTROL)
                .expect("cache-control")
                .to_str()
                .expect("utf-8"),
            "no-cache"
        );

        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body bytes");
        let body = String::from_utf8(body.to_vec()).expect("html body");
        assert!(body.contains("<div id=\"app\"></div>"));
    }

    #[tokio::test]
    async fn deep_links_return_index_and_missing_assets_404() {
        let deep_link = router()
            .oneshot(
                Request::builder()
                    .uri(Uri::from_static("/app/account"))
                    .body(axum::body::Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(deep_link.status(), StatusCode::OK);

        let missing_asset = router()
            .oneshot(
                Request::builder()
                    .uri(Uri::from_static("/app/assets/missing.js"))
                    .body(axum::body::Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(missing_asset.status(), StatusCode::NOT_FOUND);
    }
}
