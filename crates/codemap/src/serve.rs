//! `codemap serve`: the embedded web app and `POST /api/query`.

use std::io;
use std::net::{Ipv4Addr, SocketAddr};

use axum::Json;
use axum::Router;
use axum::http::{StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use codemap_core::{Answer, Query, answer};
use rust_embed::Embed;

/// The web build. The digest makes cargo recompile when `web/dist` changes.
#[derive(Embed)]
#[folder = "../../web/dist"]
struct Assets;

/// Digest of the embedded build, shown at start-up so a stale embed is visible.
const WEB_DIGEST: &str = env!("CODEMAP_WEB_DIGEST");

#[derive(Debug, thiserror::Error)]
pub enum ServeError {
    #[error("cannot listen on port {port}: {source}")]
    Bind { port: u16, source: io::Error },
    #[error("server failed: {0}")]
    Serve(io::Error),
}

pub fn router() -> Router {
    Router::new()
        .route("/api/query", post(query))
        .fallback(get(asset))
}

async fn query(Json(query): Json<Query>) -> Json<Answer> {
    Json(answer(&query))
}

/// Serves a built file, or `index.html` for any other path so client-side routes load.
async fn asset(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let (file, name) = match Assets::get(path) {
        Some(file) if !path.is_empty() => (file, path),
        _ => match Assets::get("index.html") {
            Some(file) => (file, "index.html"),
            None => return StatusCode::NOT_FOUND.into_response(),
        },
    };
    let mime = mime_for(name);
    ([(header::CONTENT_TYPE, mime)], file.data.into_owned()).into_response()
}

fn mime_for(name: &str) -> String {
    Assets::get(name).map_or_else(String::new, |file| file.metadata.mimetype().to_owned())
}

/// Runs the server until it fails.
///
/// # Errors
/// Returns an error when the port cannot be bound or the server stops abnormally.
pub fn run(port: u16) -> Result<(), ServeError> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_io()
        .build()
        .map_err(ServeError::Serve)?;
    runtime.block_on(async {
        let address = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
        let listener = tokio::net::TcpListener::bind(address)
            .await
            .map_err(|source| ServeError::Bind { port, source })?;
        let bound = listener.local_addr().map_err(ServeError::Serve)?;
        println!("codemap serving http://{bound} (web build {WEB_DIGEST})");
        axum::serve(listener, router())
            .await
            .map_err(ServeError::Serve)
    })
}

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    use super::*;

    async fn body_of(response: Response) -> Vec<u8> {
        axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body reads")
            .to_vec()
    }

    #[tokio::test]
    async fn query_answers_with_a_typed_placeholder() {
        let request = Request::post("/api/query")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(r#"{"kind":"overview"}"#))
            .expect("request builds");
        let response = router().oneshot(request).await.expect("router answers");
        assert_eq!(response.status(), StatusCode::OK);
        let json: serde_json::Value =
            serde_json::from_slice(&body_of(response).await).expect("json body");
        assert_eq!(json["kind"], "placeholder");
    }

    #[tokio::test]
    async fn unknown_paths_fall_back_to_the_app_shell_when_built() {
        let request = Request::get("/some/route")
            .body(Body::empty())
            .expect("request builds");
        let response = router().oneshot(request).await.expect("router answers");
        // Without a web build there is no index.html to fall back to.
        let expected = if Assets::get("index.html").is_some() {
            StatusCode::OK
        } else {
            StatusCode::NOT_FOUND
        };
        assert_eq!(response.status(), expected);
    }
}
