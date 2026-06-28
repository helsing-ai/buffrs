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

/// Shared registry state that includes an optional authentication token
#[derive(Clone)]
struct RegistryState {
    packages: State,
    /// When set, GET requests must include a matching Bearer token
    required_token: Option<String>,
}

#[derive(Default)]
struct RegistryStateInner {
    packages: HashMap<String, Bytes>,
    /// Tracks Maven versions per package key for serving maven-metadata.xml
    maven_versions: HashMap<String, Vec<String>>,
}

type State = Arc<RwLock<RegistryStateInner>>;

/// Run a minimal registry for local testing
async fn test_registry(
    listener: TcpListener,
    required_token: Option<String>,
) -> miette::Result<()> {
    let state = RegistryState {
        packages: Arc::new(RwLock::new(RegistryStateInner::default())),
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

// Response type for the get handler
enum GetResponse {
    Package(Bytes),
    MavenMetadata(String),
}

impl IntoResponse for GetResponse {
    fn into_response(self) -> axum::response::Response {
        match self {
            GetResponse::Package(content) => {
                ([(header::CONTENT_TYPE, "application/x-gzip")], content).into_response()
            }
            GetResponse::MavenMetadata(xml) => {
                ([(header::CONTENT_TYPE, "application/xml")], xml).into_response()
            }
        }
    }
}

// basic handler that responds with a static string
async fn get_package(
    extract::State(state): extract::State<RegistryState>,
    headers: axum::http::HeaderMap,
    extract::Path(path): extract::Path<String>,
) -> Result<GetResponse, StatusCode> {
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

    // Check if this is a Maven metadata request
    if path.ends_with("/maven-metadata.xml") {
        return serve_maven_metadata(&state.packages, &path);
    }

    let content = state
        .packages
        .read()
        .unwrap()
        .packages
        .get(&path)
        .cloned()
        .ok_or(StatusCode::NOT_FOUND)?;
    Ok(GetResponse::Package(content))
}

fn serve_maven_metadata(state: &State, path: &str) -> Result<GetResponse, StatusCode> {
    // Extract package key from path: registry/repo/package/maven-metadata.xml
    // -> registry/repo/package
    let package_key = path.trim_end_matches("/maven-metadata.xml");

    let state_guard = state.read().unwrap();
    let versions = state_guard
        .maven_versions
        .get(package_key)
        .ok_or(StatusCode::NOT_FOUND)?;

    if versions.is_empty() {
        return Err(StatusCode::NOT_FOUND);
    }

    // Get the artifact name from the path
    let artifact_id = package_key.split('/').next_back().unwrap_or("unknown");
    let latest = versions.last().unwrap();

    let metadata = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<metadata>
  <artifactId>{}</artifactId>
  <versioning>
    <latest>{}</latest>
    <release>{}</release>
    <versions>
{}
    </versions>
  </versioning>
</metadata>"#,
        artifact_id,
        latest,
        latest,
        versions
            .iter()
            .map(|v| format!("      <version>{}</version>", v))
            .collect::<Vec<_>>()
            .join("\n")
    );

    tracing::info!(
        "Serving Maven metadata for {package_key} with {} versions",
        versions.len()
    );
    Ok(GetResponse::MavenMetadata(metadata))
}

async fn put_package(
    extract::State(state): extract::State<RegistryState>,
    extract::Path(path): extract::Path<String>,
    body: Bytes,
) {
    tracing::info!("Uploaded package to {path} ({} bytes)", body.len());

    let mut state_guard = state.packages.write().unwrap();

    // Track Maven versions if this looks like a Maven package
    // Path format: registry/repo/package/version/package-version.tgz
    if let Some(version_info) = extract_maven_version(&path) {
        let versions = state_guard
            .maven_versions
            .entry(version_info.package_key.clone())
            .or_default();

        if !versions.contains(&version_info.version) {
            versions.push(version_info.version.clone());
            versions.sort();
            tracing::info!(
                "Tracked Maven version {} for {}",
                version_info.version,
                version_info.package_key
            );
        }
    }

    state_guard.packages.insert(path, body);
}

struct MavenVersionInfo {
    package_key: String,
    version: String,
}

fn extract_maven_version(path: &str) -> Option<MavenVersionInfo> {
    // Path format: registry/repo/package/version/package-version.tgz
    let parts: Vec<&str> = path.split('/').collect();
    if parts.len() < 4 {
        return None;
    }

    // Get version (second to last part)
    let version = parts[parts.len() - 2];

    // Package key is everything except the last two parts (version and filename)
    let package_key = parts[..parts.len() - 2].join("/");

    Some(MavenVersionInfo {
        package_key,
        version: version.to_string(),
    })
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

/// Run a Maven registry with metadata support for local testing
#[tokio::main]
pub async fn with_test_maven_registry<F: FnOnce(&str)>(f: F) {
    // spawn test registry in separate Tokio task
    let listen = SocketAddr::new("127.0.0.1".parse().unwrap(), 0);
    let listener = TcpListener::bind(listen).await.unwrap();
    let local_addr = listener.local_addr().unwrap();
    let handle = tokio::task::spawn(test_registry(listener, None));

    tracing::info!("Listening on Maven registry at {local_addr:?}");
    // Return URL with maven+ prefix to indicate Maven registry type
    let url = format!("maven+http://{local_addr}/registry");

    // Remove the prefix for the health check
    let check_url = url.strip_prefix("maven+").unwrap();
    wait_for_registry(check_url, &handle).await;

    // run user code
    f(&url);
}
