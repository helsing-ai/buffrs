//! Unit tests for [`Filesystem`].
//!
//! These test verify that the filesystem storage layer is implemented correctly. Every single
//! test uses a new temporary filesystem location created by [`temp_filesystem`] to ensure that
//! tests do not interfere with each other. Every single test performs some setup using manual
//! filesystem interactions, run at most one method under test, and verify the outputs and the
//! filesystem side effects.

use super::*;

use std::error::Error;
use tempfile::TempDir;

/// Create a temporary filesystem storage.
pub async fn temp_filesystem() -> (Filesystem, Cleanup) {
    let dir = TempDir::new().unwrap();
    let storage = Filesystem::new(dir.path().to_path_buf());
    let cleanup = async move {
        dir.close().unwrap();
    };
    (storage, Box::pin(cleanup))
}

#[proptest(async = "tokio")]
async fn can_write_package(
    #[strategy(package_version())] version: PackageVersion,
    contents: Vec<u8>,
) {
    with(temp_filesystem, |storage| async move {
        storage.package_put(&version, &contents).await.unwrap();

        let path = storage.path().join(version.file_name());
        let found = tokio::fs::read(&path).await.unwrap();
        assert_eq!(found, contents);
    })
    .await;
}

#[proptest(async = "tokio")]
async fn can_write_package_existing(
    #[strategy(package_version())] version: PackageVersion,
    previous: Vec<u8>,
    contents: Vec<u8>,
) {
    with(temp_filesystem, |storage| async move {
        let path = storage.path().join(version.file_name());
        tokio::fs::write(&path, &previous).await.unwrap();

        storage.package_put(&version, &contents).await.unwrap();

        let found = tokio::fs::read(&path).await.unwrap();
        assert_eq!(found, contents);
    })
    .await;
}

#[proptest(async = "tokio")]
async fn cannot_read_package_missing(#[strategy(package_version())] version: PackageVersion) {
    with(temp_filesystem, |storage| async move {
        let path = storage.path().join(version.file_name());

        let error = storage.package_get(&version).await.err().unwrap();

        assert!(matches!(error, StorageError::PackageMissing(_)));
        assert_eq!(error.to_string(), format!("package missing"));
        assert_eq!(
            error.source().unwrap().to_string(),
            format!("error writing to {path:?}")
        );
    })
    .await;
}

#[proptest(async = "tokio")]
async fn can_read_package(
    #[strategy(package_version())] version: PackageVersion,
    contents: Vec<u8>,
) {
    with(temp_filesystem, |storage| async move {
        let path = storage.path().join(version.file_name());
        tokio::fs::write(&path, &contents).await.unwrap();

        let found = storage.package_get(&version).await.unwrap();

        assert_eq!(&found[..], &contents);
    })
    .await;
}
