//! `codemap serve`: the embedded web app and `POST /api/query`.

use std::io;
use std::net::{Ipv4Addr, SocketAddr};

use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::http::{StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use codemap_core::{Envelope, Index, IndexError, Query, QueryError, answer, index};
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
    #[error(transparent)]
    Index(#[from] IndexError),
}

pub fn router(root: PathBuf) -> Router {
    Router::new()
        .route("/api/query", post(query))
        .fallback(get(asset))
        .with_state(Arc::new(root))
}

/// Answers a query from the index under the served root. A bad query is the caller's fault
/// (400); anything else is the server's (500).
async fn query(State(root): State<Arc<PathBuf>>, Json(query): Json<Query>) -> Response {
    let result = tokio::task::spawn_blocking(move || {
        let index = Index::open(&root)?;
        answer(&index, &query)
    })
    .await;
    match result {
        Ok(Ok(answer)) => Json(Envelope::from(answer)).into_response(),
        Ok(Err(error)) => {
            let status = match error {
                QueryError::UnknownMeasure(_)
                | QueryError::SizeIsRatio
                | QueryError::UnknownArea(_)
                | QueryError::UnknownValue { .. } => StatusCode::BAD_REQUEST,
                _ => StatusCode::INTERNAL_SERVER_ERROR,
            };
            (status, error.to_string()).into_response()
        }
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
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
pub fn run(root: &Path, port: u16) -> Result<(), ServeError> {
    index(root)?;
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
        axum::serve(listener, router(root.to_owned()))
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

    fn served_tree() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("temp dir");
        std::fs::write(dir.path().join("a.py"), "def f():\n    return 1\n").expect("write");
        index(dir.path()).expect("index");
        dir
    }

    async fn post(root: &Path, body: &'static str) -> Response {
        let request = Request::post("/api/query")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body))
            .expect("request builds");
        router(root.to_owned())
            .oneshot(request)
            .await
            .expect("router answers")
    }

    #[tokio::test]
    async fn query_answers_from_the_index_in_the_envelope() {
        let dir = served_tree();
        let response = post(dir.path(), r#"{"kind":"overview"}"#).await;
        assert_eq!(response.status(), StatusCode::OK);
        let json: serde_json::Value =
            serde_json::from_slice(&body_of(response).await).expect("json body");
        assert_eq!(json["schema"], codemap_core::SCHEMA);
        assert_eq!(json["kind"], "overview");
        assert_eq!(json["totals"]["lines"], 2);
    }

    #[tokio::test]
    async fn a_bad_query_is_a_client_error() {
        let dir = served_tree();
        let response = post(dir.path(), r#"{"kind":"area","subject":"nowhere"}"#).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn unknown_paths_fall_back_to_the_app_shell_when_built() {
        let request = Request::get("/some/route")
            .body(Body::empty())
            .expect("request builds");
        let response = router(PathBuf::new())
            .oneshot(request)
            .await
            .expect("router answers");
        // Without a web build there is no index.html to fall back to.
        let expected = if Assets::get("index.html").is_some() {
            StatusCode::OK
        } else {
            StatusCode::NOT_FOUND
        };
        assert_eq!(response.status(), expected);
    }
}
