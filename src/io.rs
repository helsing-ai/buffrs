use miette::{Context, IntoDiagnostic};
use std::path::Path;
use tokio::fs;

use crate::errors::FileExistsError;

/// The `File` trait standardizes the process of reading and writing files.
#[async_trait::async_trait]
pub trait File: Sized + Send + Sync + 'static {
    /// The default location of this file
    const DEFAULT_PATH: &str;

    /// Checks if the file currently exists in the filesystem at its default path
    async fn exists() -> miette::Result<bool> {
        Self::exists_at(Self::DEFAULT_PATH).await
    }

    /// Checks if the file currently exists in the filesystem at a given path
    async fn exists_at<P>(path: P) -> miette::Result<bool>
    where
        P: AsRef<Path> + Send + Sync,
    {
        fs::try_exists(path)
            .await
            .into_diagnostic()
            .wrap_err(FileExistsError(Self::DEFAULT_PATH))
    }

    /// Loads the file from the current directory
    async fn read() -> miette::Result<Self> {
        Self::read_from(Self::DEFAULT_PATH).await
    }

    /// Loads the file from a specific path.
    async fn read_from<P>(path: P) -> miette::Result<Self>
    where
        P: AsRef<Path> + Send + Sync;

    /// Loads the file from the current directory, if it exists, otherwise returns an empty one. Fails, if the exists() check fails
    async fn read_or_default() -> miette::Result<Self>
    where
        Self: Default,
    {
        if Self::exists().await? {
            Self::read().await
        } else {
            Ok(Self::default())
        }
    }

    /// Loads the file from a specific path, if it exists, otherwise returns an empty one. Fails, if the exists() check fails
    async fn read_from_or_default<P>(path: P) -> miette::Result<Self>
    where
        Self: Default,
        P: AsRef<Path> + Send + Sync,
    {
        if Self::exists_at(&path).await? {
            Self::read_from(path).await
        } else {
            Ok(Self::default())
        }
    }

    /// Persists a file to the filesystem
    async fn write<P>(&self, path: P) -> miette::Result<()>
    where
        P: AsRef<Path> + Send + Sync;
}
