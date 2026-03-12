use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use axum::{
    Router, extract,
    http::{StatusCode, header},
    response::IntoResponse,
    routing::get,
};
use bytes::Bytes;
use miette::{Context as _, IntoDiagnostic, miette};
use tokio::net::TcpListener;

type State = Arc<RwLock<HashMap<String, Bytes>>>;

/// Run a minimal registry for local testing
async fn test_registry(listener: TcpListener) -> miette::Result<()> {
    let state = Arc::new(RwLock::new(HashMap::<String, Bytes>::new()));
    let app = Router::new()
        .route("/{*path}", get(get_package).put(put_package))
        .with_state(state);
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
    // spawn test registry in separate Tokio task
    let listen = SocketAddr::new("127.0.0.1".parse().unwrap(), 0);
    let listener = TcpListener::bind(listen).await.unwrap();
    let local_addr = listener.local_addr().unwrap();
    let handle = tokio::task::spawn(test_registry(listener));

    tracing::info!("Listening on {local_addr:?}");
    let url = format!("http://{local_addr}/registry");

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
