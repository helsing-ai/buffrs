use crate::{credentials::Credentials, package::PackageStore};
use miette::IntoDiagnostic;
use std::path::PathBuf;
use std::sync::Arc;

/// Shared context representing `buffrs` project.
#[derive(Debug)]
pub struct Context {
    /// Package store, contains installed dependencies.
    store: PackageStore,
    /// Credentials, used to access registries.
    credentials: Arc<Credentials>,
}

impl Context {
    /// Package store
    pub fn store(&self) -> &PackageStore {
        &self.store
    }

    /// Parsed credentials
    pub fn credentials(&self) -> &Arc<Credentials> {
        &self.credentials
    }

    /// Create new context.
    ///
    /// This will create and initialize a new `buffrs` project.
    pub async fn create<P: Into<PathBuf>>(path: P) -> miette::Result<Arc<Self>> {
        Ok(Arc::new(Self {
            store: PackageStore::create(path.into()).await?,
            credentials: Credentials::load().await.map(Arc::new)?,
        }))
    }

    /// Open a context by path.
    ///
    /// This will check if the path is a valid `buffrs` project, and return an error if it is not.
    pub async fn open<P: Into<PathBuf>>(path: P) -> miette::Result<Arc<Self>> {
        Ok(Arc::new(Self {
            store: PackageStore::open(&path.into()).await?,
            credentials: Credentials::read().await?.map(Arc::new).unwrap_or_default(),
        }))
    }

    /// Open a context in the current working directory.
    pub async fn open_current() -> miette::Result<Arc<Self>> {
        let current = std::env::current_dir().into_diagnostic()?;
        Self::open(current).await
    }
}
