// Copyright 2023 Helsing GmbH
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use super::*;
use std::{
    fmt::Debug,
    io::ErrorKind,
    path::{Path, PathBuf},
};
use tokio::{fs::OpenOptions, io::AsyncWriteExt};

/// Filesystem-backed storage for packages.
///
/// This storage layer needs a root path, which should be a folder. It will store packages as files
/// in the root path, named after the package name and version.
///
/// For example, if the root path is `/path/to/storage`, then a package might be stored at
/// `/path/to/storage/mypackage_0.1.5.tar.gz`. See [`Filesystem::package_path()`] for more
/// information.
///
/// # Examples
///
/// Create a new filesystem storage instance:
///
/// ```rust
/// # use buffrs_registry::{types::PackageVersion, storage::Filesystem};
/// # use std::path::Path;
/// let filesystem = Filesystem::new("/path/to/storage");
///
/// let package = PackageVersion {
///     package: "mypackage".parse().unwrap(),
///     version: "0.1.5".parse().unwrap(),
/// };
///
/// assert_eq!(filesystem.package_path(&package), Path::new("/path/to/storage/mypackage_0.1.5.tar.gz"));
/// ```
#[derive(Clone, Debug)]
pub struct Filesystem<P: AsRef<Path> = PathBuf> {
    path: P,
}

/// Error interacting with the filesystem.
///
/// This error adds some context to the underlying [`std::io::Error`], such as the path that was
/// being written to.
#[derive(thiserror::Error, Debug)]
#[error("error writing to {path:?}")]
pub struct FilesystemError {
    /// Path that was being written to or read from.
    path: PathBuf,
    /// Error that occurred.
    #[source]
    error: std::io::Error,
}

impl<P: AsRef<Path>> Filesystem<P> {
    /// Create new Filesystem storage instance.
    pub fn new(path: P) -> Self {
        Self { path }
    }

    /// Get the base path of this filesystem storage instance.
    pub fn path(&self) -> &Path {
        self.path.as_ref()
    }

    /// Get the full path of a package version.
    ///
    /// Uses [`PackageVersion::file_name()`] to determine the file name.
    pub fn package_path(&self, version: &PackageVersion) -> PathBuf {
        self.path().join(version.file_name())
    }

    async fn do_package_put(
        &self,
        version: &PackageVersion,
        data: &[u8],
    ) -> Result<(), FilesystemError> {
        let path = self.package_path(version);
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&path)
            .await
            .map_err(|error| FilesystemError {
                path: path.clone(),
                error,
            })?;
        file.write_all(data)
            .await
            .map_err(|error| FilesystemError {
                path: path.clone(),
                error,
            })?;
        file.flush()
            .await
            .map_err(|error| FilesystemError { path, error })?;
        Ok(())
    }

    async fn do_package_get(&self, version: &PackageVersion) -> Result<Bytes, FilesystemError> {
        let path = self.package_path(version);
        tokio::fs::read(&path)
            .await
            .map(Into::into)
            .map_err(|error| FilesystemError { path, error })
    }
}

#[async_trait::async_trait]
impl<P: AsRef<Path> + Send + Sync + Debug> Storage for Filesystem<P> {
    async fn package_put(&self, version: &PackageVersion, data: &[u8]) -> Result<(), StorageError> {
        self.do_package_put(version, data)
            .await
            .map_err(|error| StorageError::Other(Arc::new(error)))
    }

    async fn package_get(&self, version: &PackageVersion) -> Result<Bytes, StorageError> {
        let result = self.do_package_get(version).await;
        match result {
            Ok(bytes) => Ok(bytes),
            Err(error) if error.error.kind() == ErrorKind::NotFound => {
                Err(StorageError::PackageMissing(Arc::new(error)))
            }
            Err(error) => Err(StorageError::Other(Arc::new(error))),
        }
    }
}
