use std::path::{Path, PathBuf};

use miette::{Context, IntoDiagnostic};
use tokio::fs;

use crate::errors::FileExistsError;

/// The `File` trait standardizes the process of reading and writing files.
#[async_trait::async_trait]
pub trait File: Sized + Send + Sync + 'static {
    /// The default location of this file
    const DEFAULT_PATH: &str;

    fn resolve<P>(path: P) -> miette::Result<PathBuf>
    where
        P: AsRef<Path> + Send + Sync,
    {
        let path = path.as_ref();

        if path.is_file() {
            return Ok(path.to_path_buf());
        }

        let parent = path.parent().ok_or(miette::miette!(
            "Parent not resolvable for {}",
            path.display()
        ))?;

        Ok(parent.join(Self::DEFAULT_PATH))
    }

    /// Checks if the file currently exists in the filesystem at its default path
    async fn exists() -> miette::Result<bool> {
        Self::exists_at(Self::DEFAULT_PATH).await
    }

    /// Checks if the file currently exists in the filesystem at a given path
    async fn exists_at<P>(path: P) -> miette::Result<bool>
    where
        P: AsRef<Path> + Send + Sync,
    {
        let resolved = Self::resolve(path)?;

        fs::try_exists(resolved)
            .await
            .into_diagnostic()
            .wrap_err(FileExistsError(Self::DEFAULT_PATH))
    }

    /// Loads the file from the current directory
    async fn load() -> miette::Result<Self> {
        Self::load_from(Self::DEFAULT_PATH).await
    }

    /// Loads the file from a specific path.
    async fn load_from<P>(path: P) -> miette::Result<Self>
    where
        P: AsRef<Path> + Send + Sync;

    /// Loads the file from the current directory, if it exists, otherwise returns an empty one. Fails, if the exists() check fails
    async fn load_or_default() -> miette::Result<Self>
    where
        Self: Default,
    {
        if Self::exists().await? {
            Self::load().await
        } else {
            Ok(Self::default())
        }
    }

    /// Loads the file from a specific path, if it exists, otherwise returns an empty one. Fails, if the exists() check fails
    async fn load_from_or_default<P>(path: P) -> miette::Result<Self>
    where
        Self: Default,
        P: AsRef<Path> + Send + Sync,
    {
        let resolved = Self::resolve(path)?;

        if Self::exists_at(&resolved).await? {
            Self::load_from(resolved).await
        } else {
            Ok(Self::default())
        }
    }

    /// Persists a file to the filesystem.
    async fn save<P>(&self, path: P) -> miette::Result<()>
    where
        P: AsRef<Path> + Send + Sync;
}
