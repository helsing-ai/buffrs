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

/// Shared registry state that includes an optional authentication token
#[derive(Clone)]
struct RegistryState {
    packages: State,
    /// When set, GET requests must include a matching Bearer token
    required_token: Option<String>,
}

/// Run a minimal registry for local testing
async fn test_registry(
    listener: TcpListener,
    required_token: Option<String>,
) -> miette::Result<()> {
    let state = RegistryState {
        packages: Arc::new(RwLock::new(HashMap::<String, Bytes>::new())),
        required_token,
    };
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
    extract::State(state): extract::State<RegistryState>,
    headers: axum::http::HeaderMap,
    extract::Path(path): extract::Path<String>,
) -> Result<impl IntoResponse, StatusCode> {
    // Check authentication if required
    if let Some(ref expected_token) = state.required_token {
        let authorized = headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v == format!("Bearer {expected_token}"));

        if !authorized {
            tracing::info!("Rejected unauthenticated GET for {path}");
            return Err(StatusCode::UNAUTHORIZED);
        }
    }

    tracing::info!("Downloaded package from {path}");
    let content = state
        .packages
        .read()
        .unwrap()
        .get(&path)
        .cloned()
        .ok_or(StatusCode::NOT_FOUND)?;
    Ok(([(header::CONTENT_TYPE, "application/x-gzip")], content))
}

async fn put_package(
    extract::State(state): extract::State<RegistryState>,
    extract::Path(path): extract::Path<String>,
    body: Bytes,
) {
    tracing::info!("Uploaded package to {path} ({} bytes)", body.len());
    state.packages.write().unwrap().insert(path, body);
}

async fn wait_for_registry(url: &str, handle: &tokio::task::JoinHandle<miette::Result<()>>) {
    let dur = Duration::from_millis(10);
    let client = reqwest::Client::builder()
        .connect_timeout(dur)
        .build()
        .unwrap();
    loop {
        // perform a simple request at an arbitrary URL to check readiness
        // note: for authenticated registries this returns 401, which is still Ok
        if client.get(url).send().await.is_ok() {
            break;
        }
        // check whether the test registry has failed instead of looping indefinitely
        assert!(!handle.is_finished(), "test registry ended unexpectedly");
        // no busy wait
        tokio::time::sleep(dur).await;
    }
}

// do not use (flavor = "current_thread") here because the user-provided function is blocking
#[tokio::main]
pub async fn with_test_registry<F: FnOnce(&str)>(f: F) {
    // spawn test registry in separate Tokio task
    let listen = SocketAddr::new("127.0.0.1".parse().unwrap(), 0);
    let listener = TcpListener::bind(listen).await.unwrap();
    let local_addr = listener.local_addr().unwrap();
    let handle = tokio::task::spawn(test_registry(listener, None));

    tracing::info!("Listening on {local_addr:?}");
    let url = format!("http://{local_addr}/registry");

    wait_for_registry(&url, &handle).await;

    // run user code
    f(&url);
}

/// Like `with_test_registry`, but GET requests require a Bearer token.
/// The callback receives both the registry URL and the token to use.
#[tokio::main]
pub async fn with_authenticated_test_registry<F: FnOnce(&str, &str)>(f: F) {
    let token = "test-registry-token";

    let listen = SocketAddr::new("127.0.0.1".parse().unwrap(), 0);
    let listener = TcpListener::bind(listen).await.unwrap();
    let local_addr = listener.local_addr().unwrap();
    let handle = tokio::task::spawn(test_registry(listener, Some(token.to_owned())));

    tracing::info!("Listening on {local_addr:?} (authenticated)");
    let url = format!("http://{local_addr}/registry");

    wait_for_registry(&url, &handle).await;

    // run user code
    f(&url, token);
}
