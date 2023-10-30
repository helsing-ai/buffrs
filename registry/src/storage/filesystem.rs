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
use tokio::{
    fs::{File, OpenOptions},
    io::AsyncWriteExt,
};

/// Filesystem-backed storage for packages.
#[derive(Clone, Debug)]
pub struct Filesystem<P: AsRef<Path>> {
    path: P,
}

#[derive(thiserror::Error, Debug)]
#[error("error writing to {path:?}")]
pub struct FilesystemError {
    path: PathBuf,
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

    fn package_path(&self, name: &str, version: &str) -> PathBuf {
        self.path().join(format!("{name}-{version}.tar.gz"))
    }

    async fn do_package_put(
        &self,
        name: &str,
        version: &str,
        data: &[u8],
    ) -> Result<(), FilesystemError> {
        let path = self.package_path(name, version);
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
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

    async fn do_package_get(&self, name: &str, version: &str) -> Result<Bytes, FilesystemError> {
        let path = self.package_path(name, version);
        tokio::fs::read(&name)
            .await
            .map(Into::into)
            .map_err(|error| FilesystemError { path, error })
    }
}

#[async_trait::async_trait]
impl<P: AsRef<Path> + Send + Sync + Debug> Storage for Filesystem<P> {
    async fn package_put(&self, version: &PackageVersion, data: &[u8]) -> Result<(), StorageError> {
        match self
            .do_package_put(&version.package, &version.version, data)
            .await
        {
            Ok(()) => Ok(()),
            Err(error) => todo!(),
        }
    }

    async fn package_get(&self, version: &PackageVersion) -> Result<Bytes, StorageError> {
        let result = self
            .do_package_get(&version.package, &version.version)
            .await;
        match result {
            Ok(bytes) => Ok(bytes),
            Err(error) if error.error.kind() == ErrorKind::NotFound => {
                Err(StorageError::PackageMissing)
            }
            Err(error) => Err(StorageError::Other(Arc::new(error))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempdir::TempDir;

    #[tokio::test]
    async fn can_write_package() {
        let dir = TempDir::new("storage").unwrap();
        let storage = Filesystem::new(dir.path());
        let contents = b"0xdeadbeef";

        storage
            .do_package_put("mypackage", "0.1.5", contents)
            .await
            .unwrap();

        let path = storage.path().join("mypackage-0.1.5.tar.gz");
        assert!(tokio::fs::try_exists(&path).await.unwrap());
        let found = tokio::fs::read(&path).await.unwrap();
        assert_eq!(found, contents);
    }

    #[tokio::test]
    async fn cannot_write_package_existing() {
        let dir = TempDir::new("storage").unwrap();
        let storage = Filesystem::new(dir.path());
        let contents = b"0xdeadbeef";

        let path = storage.path().join("mypackage-0.1.5.tar.gz");
        tokio::fs::write(&path, contents).await.unwrap();

        let error = storage
            .do_package_put("mypackage", "0.1.5", contents)
            .await
            .err()
            .unwrap();

        assert_eq!(error.path, path);
        assert_eq!(error.error.kind(), ErrorKind::AlreadyExists);
    }

    #[tokio::test]
    async fn cannot_read_package_missing() {
        let dir = TempDir::new("storage").unwrap();
        let storage = Filesystem::new(dir.path());
        let contents = b"0xdeadbeef";

        let path = storage.path().join("mypackage-0.1.5.tar.gz");

        let error = storage
            .do_package_get("mypackage", "0.1.5")
            .await
            .err()
            .unwrap();

        assert_eq!(error.path, path);
        assert_eq!(error.error.kind(), ErrorKind::NotFound);
    }

    #[tokio::test]
    async fn can_read_package() {
        let dir = TempDir::new("storage").unwrap();
        let storage = Filesystem::new(dir.path());
        let contents = b"0xdeadbeef";

        let path = storage.path().join("mypackage-0.1.5.tar.gz");
        tokio::fs::write(&path, contents).await.unwrap();

        let found = storage.do_package_get("mypackage", "0.1.5").await.unwrap();

        assert_eq!(&found[..], contents);
    }
}
