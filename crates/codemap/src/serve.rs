//! `codemap serve`: the embedded web app, `POST /api/query`, and `GET /api/events`.

use std::io;
use std::net::{Ipv4Addr, SocketAddr};

use std::convert::Infallible;
use std::path::Path;
use std::sync::{Arc, Mutex};

use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::http::{StatusCode, Uri, header};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use codemap_core::{
    Envelope, Index, IndexError, Query, QueryError, Refresh, SemanticStatus, answer,
};
use rust_embed::Embed;
use tokio::sync::watch;
use tokio_stream::StreamExt as _;
use tokio_stream::wrappers::WatchStream;

use crate::watch::{WatchError, Watcher, watch as watch_tree};

/// The web build, embedded; `build.rs` recompiles it when `web/dist` changes.
#[derive(Embed)]
#[folder = "../../web/dist"]
struct Assets;

#[derive(Debug, thiserror::Error)]
pub enum ServeError {
    #[error("cannot listen on port {port}: {source}")]
    Bind { port: u16, source: io::Error },
    #[error("server failed: {0}")]
    Serve(io::Error),
    #[error(transparent)]
    Index(#[from] IndexError),
    #[error(transparent)]
    Watch(#[from] WatchError),
}

/// A served project: the open index its requests read, and the revision of what they read.
pub struct Served {
    /// Requests read through clones of this handle. Opening the file per request would be a
    /// separate database instance that never sees the watcher's writes; a clone shares the
    /// instance, and `DuckDB`'s snapshot isolation keeps it on the previous state while a
    /// re-index is writing.
    readers: Mutex<Index>,
    /// Counts completed indexes since start; the first is the one made at start.
    revision: Arc<watch::Sender<u32>>,
    _watcher: Watcher,
}

impl Served {
    /// Indexes `root` (seeding a worktree first), then keeps it fresh until dropped.
    ///
    /// # Errors
    /// Fails when the tree cannot be indexed or watched.
    pub fn start(root: &Path) -> Result<Self, ServeError> {
        let index = Index::create(root)?;
        let revision = Arc::new(watch::channel(1).0);
        let announced = Arc::clone(&revision);
        let announced_semantic = Arc::clone(&revision);
        let watcher = watch_tree(
            index.try_clone().map_err(IndexError::from)?,
            move |result| match result {
                Ok(_) => announced.send_modify(|revision| *revision += 1),
                Err(error) => eprintln!("codemap: re-index failed: {error}"),
            },
            move |result| match result {
                // Nothing was written, or the run will be made again.
                Ok(
                    SemanticStatus::Off(_)
                    | SemanticStatus::Unchanged
                    | SemanticStatus::Pending
                    | SemanticStatus::Superseded,
                ) => {}
                Ok(_) => announced_semantic.send_modify(|revision| *revision += 1),
                Err(error) => eprintln!("codemap: semantic duplication failed: {error}"),
            },
        )?;
        Ok(Self {
            readers: Mutex::new(index),
            revision,
            _watcher: watcher,
        })
    }

    /// Subscribes to revisions: the current one first, then each completed re-index.
    #[must_use]
    pub fn subscribe(&self) -> watch::Receiver<u32> {
        self.revision.subscribe()
    }

    fn reader(&self) -> Result<Index, QueryError> {
        self.readers
            .lock()
            .expect("the lock is held only to clone a handle")
            .try_clone()
    }
}

pub fn router(served: Arc<Served>) -> Router {
    Router::new()
        .route("/api/query", post(query))
        .route("/api/events", get(events))
        .fallback(get(asset))
        .with_state(served)
}

/// A server-sent `refresh` event with the current revision on connecting and another after each
/// completed re-index. A burst of re-indexes may arrive as its latest revision only.
async fn events(
    State(served): State<Arc<Served>>,
) -> Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>> {
    let stream = WatchStream::new(served.subscribe()).map(|revision| {
        Ok(Event::default()
            .event("refresh")
            .json_data(Refresh { revision })
            .expect("a revision always encodes"))
    });
    Sse::new(stream).keep_alive(KeepAlive::default())
}

/// Answers a query from the index under the served root. A bad query is the caller's fault
/// (400); anything else is the server's (500).
async fn query(State(served): State<Arc<Served>>, Json(query): Json<Query>) -> Response {
    let result = tokio::task::spawn_blocking(move || answer(&served.reader()?, &query)).await;
    match result {
        Ok(Ok(answer)) => Json(Envelope::from(answer)).into_response(),
        Ok(Err(error)) => {
            let status = match error {
                QueryError::UnknownMeasure(_)
                | QueryError::SizeIsRatio
                | QueryError::UnknownArea(_)
                | QueryError::UnknownValue { .. }
                | QueryError::OutsideRoot(_)
                | QueryError::UnknownFile(_)
                | QueryError::NotText(_)
                | QueryError::BadRange => StatusCode::BAD_REQUEST,
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
    let served = Arc::new(Served::start(root)?);
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(ServeError::Serve)?;
    runtime.block_on(async {
        let address = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
        let listener = tokio::net::TcpListener::bind(address)
            .await
            .map_err(|source| ServeError::Bind { port, source })?;
        let bound = listener.local_addr().map_err(ServeError::Serve)?;
        println!("codemap {} serving http://{bound}", crate::VERSION);
        axum::serve(listener, router(served))
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

    fn served_tree() -> (tempfile::TempDir, Arc<Served>) {
        let dir = tempfile::tempdir().expect("temp dir");
        std::fs::write(dir.path().join("a.py"), "def f():\n    return 1\n").expect("write");
        let served = Arc::new(Served::start(dir.path()).expect("serves"));
        (dir, served)
    }

    async fn request(served: &Arc<Served>, request: Request<Body>) -> Response {
        router(Arc::clone(served))
            .oneshot(request)
            .await
            .expect("router answers")
    }

    async fn post(served: &Arc<Served>, body: &'static str) -> Response {
        let query = Request::post("/api/query")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body))
            .expect("request builds");
        request(served, query).await
    }

    async fn line_count(served: &Arc<Served>) -> u64 {
        let response = post(served, r#"{"kind":"overview"}"#).await;
        assert_eq!(response.status(), StatusCode::OK);
        let json: serde_json::Value =
            serde_json::from_slice(&body_of(response).await).expect("json body");
        json["totals"]["lines"].as_u64().expect("a line count")
    }

    #[tokio::test]
    async fn query_answers_from_the_index_in_the_envelope() {
        let (_dir, served) = served_tree();
        let response = post(&served, r#"{"kind":"overview"}"#).await;
        assert_eq!(response.status(), StatusCode::OK);
        let json: serde_json::Value =
            serde_json::from_slice(&body_of(response).await).expect("json body");
        assert_eq!(json["schema"], codemap_core::SCHEMA);
        assert_eq!(json["kind"], "overview");
        assert_eq!(json["totals"]["lines"], 2);
    }

    #[tokio::test]
    async fn a_bad_query_is_a_client_error() {
        let (_dir, served) = served_tree();
        let response = post(&served, r#"{"kind":"area","subject":"nowhere"}"#).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn unknown_paths_fall_back_to_the_app_shell_when_built() {
        let (_dir, served) = served_tree();
        let fallback = Request::get("/some/route")
            .body(Body::empty())
            .expect("request builds");
        let response = request(&served, fallback).await;
        // Without a web build there is no index.html to fall back to.
        let expected = if Assets::get("index.html").is_some() {
            StatusCode::OK
        } else {
            StatusCode::NOT_FOUND
        };
        assert_eq!(response.status(), expected);
    }

    #[tokio::test]
    async fn an_edit_under_a_running_server_changes_the_answers_and_the_revision() {
        let (dir, served) = served_tree();
        assert_eq!(*served.subscribe().borrow(), 1);
        assert_eq!(line_count(&served).await, 2);
        let mut revisions = served.subscribe();

        std::fs::write(dir.path().join("a.py"), "def f():\n    return 1\n\nx = 2\n").expect("edit");
        revisions
            .wait_for(|revision| *revision > 1)
            .await
            .expect("the server is running");

        assert_eq!(*served.subscribe().borrow(), 2);
        assert_eq!(line_count(&served).await, 4);
    }

    #[tokio::test]
    async fn an_edit_to_the_config_refreshes_too() {
        let (dir, served) = served_tree();
        let mut revisions = served.subscribe();

        std::fs::write(dir.path().join(".codemap/config.toml"), "# no rules yet\n").expect("edit");

        revisions
            .wait_for(|revision| *revision > 1)
            .await
            .expect("the server is running");
    }

    /// The `data:` payload of the next server-sent event on the stream.
    async fn next_revision(
        events: &mut (impl tokio_stream::Stream<Item = Result<axum::body::Bytes, axum::Error>> + Unpin),
    ) -> u32 {
        let chunk = events
            .next()
            .await
            .expect("the stream continues")
            .expect("a chunk");
        let text = String::from_utf8(chunk.to_vec()).expect("utf-8");
        let data = text
            .lines()
            .find_map(|line| line.strip_prefix("data:"))
            .expect("an event with data");
        let json: serde_json::Value = serde_json::from_str(data.trim()).expect("json data");
        u32::try_from(json["revision"].as_u64().expect("a revision")).expect("fits")
    }

    #[tokio::test]
    async fn the_event_stream_announces_the_revision_and_each_re_index() {
        let (dir, served) = served_tree();
        let response = request(
            &served,
            Request::get("/api/events")
                .body(Body::empty())
                .expect("request builds"),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let mut events = response.into_body().into_data_stream();
        assert_eq!(next_revision(&mut events).await, 1);

        std::fs::write(dir.path().join("b.py"), "y = 1\n").expect("add a file");

        assert_eq!(next_revision(&mut events).await, 2);
    }
}
