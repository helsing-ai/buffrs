use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Duration;

use axum::{
    extract,
    http::{header, StatusCode},
    response::IntoResponse,
    routing::get,
    Router,
};
use bytes::Bytes;
use miette::{miette, Context as _, IntoDiagnostic};

// in case 4367 is already in use, start with the next port
static PORT: AtomicU16 = AtomicU16::new(4368);

type State = Arc<RwLock<HashMap<String, Bytes>>>;

/// Run a minimal registry for local testing
async fn test_registry(listen: SocketAddr) -> miette::Result<()> {
    let state = Arc::new(RwLock::new(HashMap::<String, Bytes>::new()));
    let app = Router::new()
        .route("/*path", get(get_package).put(put_package))
        .with_state(state);
    let listener = tokio::net::TcpListener::bind(listen)
        .await
        .into_diagnostic()
        .wrap_err(miette!("failed to listen on {listen:?}"))?;
    tracing::info!("Listening on {listen:?}");
    axum::serve(listener, app)
        .await
        .into_diagnostic()
        .wrap_err(miette!("failed to read the token from the user"))
}

// basic handler that responds with a static string
async fn get_package(
    extract::State(state): extract::State<State>,
    extract::Path(path): extract::Path<String>,
) -> Result<impl IntoResponse, StatusCode> {
    tracing::info!("Downloaded package from {path}");
    let content = state
        .read()
        .unwrap()
        .get(&path)
        .cloned()
        .ok_or(StatusCode::NOT_FOUND)?;
    Ok(([(header::CONTENT_TYPE, "application/x-gzip")], content))
}

async fn put_package(
    extract::State(state): extract::State<State>,
    extract::Path(path): extract::Path<String>,
    body: Bytes,
) {
    tracing::info!("Uploaded package to {path} ({} bytes)", body.len());
    state.write().unwrap().insert(path, body);
}

// do not use (flavor = "current_thread") here because the user-provided function is blocking
#[tokio::main]
pub async fn with_test_registry<F: FnOnce(&str)>(f: F) {
    let port = PORT.fetch_add(1, Ordering::Relaxed);
    let listen = SocketAddr::new("127.0.0.1".parse().unwrap(), port);
    let url = format!("http://{listen}/registry");

    // spawn test registry in separate process
    let handle = tokio::task::spawn(test_registry(listen));

    // wait until the test registry is ready
    let dur = Duration::from_millis(10);
    let client = reqwest::Client::builder()
        .connect_timeout(dur)
        .build()
        .unwrap();
    loop {
        // perform a simple request at an arbitrary URL to check readiness
        if client.get(&url).send().await.is_ok() {
            break;
        }
        // check whether the test registry has failed instead of looping indefinitely
        assert!(!handle.is_finished(), "test registry ended unexpectedly");
        // no busy wait
        tokio::time::sleep(dur).await;
    }

    // run user code
    f(&url);
}
