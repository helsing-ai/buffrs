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

use std::{
    io::ErrorKind,
    path::{Path, PathBuf},
    str::FromStr,
};

use bytes::Bytes;
use miette::{miette, Context, IntoDiagnostic};
use walkdir::WalkDir;

use crate::{
    lock::{Digest, DigestAlgorithm, FileRequirement},
    package::{Package, PackageName},
};

/// The environment variable that overrides the default cache location
const CACHE_ENV_VAR: &str = "BUFFRS_CACHE";
/// The default cache directory name
const CACHE_DIRECTORY: &str = "cache";

/// A instance of a cache
pub struct Cache(PathBuf);

impl Cache {
    /// Open the cache
    pub async fn open() -> miette::Result<Self> {
        if let Ok(cache) = std::env::var(CACHE_ENV_VAR).map(PathBuf::from) {
            let res = tokio::fs::create_dir_all(&cache).await;

            match res {
                Ok(()) => (),
                // If the filesystem entry already exists, check if it's a directory,
                // this allow us to give a nicer error message
                Err(err) if err.kind() == ErrorKind::AlreadyExists => {
                    let is_dir = tokio::fs::metadata(&cache)
                        .await
                        .into_diagnostic()
                        .wrap_err_with(|| miette!(
                            "internal: failed to get metadata for cache dir set by {CACHE_ENV_VAR} ({})",
                            cache.display()
                        ))?
                        .is_dir();

                    if !is_dir {
                        return Err(miette!(
                            "internal: failed to initialize cache dir set by {CACHE_ENV_VAR}: '{}' exists but is not directory",
                            cache.display()
                        ));
                    }
                }
                other => other.into_diagnostic().wrap_err_with(|| {
                    miette!(
                        "internal: failed to initialize cache dir set by {CACHE_ENV_VAR} ({})",
                        cache.display()
                    )
                })?,
            }

            let path = tokio::fs::canonicalize(cache)
                .await
                .into_diagnostic()
                .wrap_err("failed to canonicalize cache directory")?;

            let cache = Self::new(path).await?;

            return Ok(cache);
        }

        let path = crate::home()?.join(CACHE_DIRECTORY);

        let cache = Self::new(path).await?;

        Ok(cache)
    }

    /// Create a new buffrs cache at a given location
    ///
    /// This function is idempotent so multiple invocations of the same path
    /// will not modify the filesystem contents.
    pub async fn new(path: PathBuf) -> miette::Result<Self> {
        let exists = tokio::fs::try_exists(&path).await.into_diagnostic()?;

        if !exists {
            tokio::fs::create_dir_all(&path).await.ok();
        }

        let cache = Self(path);

        cache.homogenize().await?;

        Ok(cache)
    }

    /// Homogenize the cache contents to adhere to the cache specification of buffrs
    ///
    /// Note: This function removes all malformed contents of the cache in an idempotent manner.
    /// Please be cautions when calling this function on arbitrary directories as subcontents may
    /// be removed.
    pub async fn homogenize(&self) -> miette::Result<()> {
        let dir = WalkDir::new(self.path())
            .max_depth(1)
            .into_iter()
            .filter_map(|e| e.ok());

        let (dirs, files): (Vec<_>, Vec<_>) = dir.partition(|e| e.path().is_dir());

        let invalid_dirs = dirs.into_iter().filter(|d| d.path() != self.path());

        for dir in invalid_dirs {
            tracing::debug!("removing invalid cache entry: {}", dir.path().display());

            tokio::fs::remove_dir_all(dir.path())
                .await
                .into_diagnostic()
                .wrap_err_with(|| miette!(
                    "cache contained an unexpected subdirectory ({}) and buffrs was unable to clean it up",
                    dir.path().display()
                ))?;
        }

        let invalid_files = files.into_iter().filter(|f| {
            let filename = f.path().file_name().unwrap_or_default().to_string_lossy();

            let parts: Vec<_> = filename.split('.').collect();

            // invalid – we should have: {name}.{type}.{digest}.{ext}
            let &[name, r#type, digest, ext] = parts.as_slice() else {
                return true;
            };

            // package name part is invalid
            if PackageName::new(name).is_err() {
                return true;
            }

            // unknown / unsupported digest algorithm
            let Ok(algorithm) = DigestAlgorithm::from_str(r#type) else {
                return true;
            };

            // invalid digest
            if Digest::from_parts(algorithm, digest).is_err() {
                return true;
            }

            // invalid extension
            if ext != "tgz" {
                return true;
            }

            false
        });

        for file in invalid_files {
            tracing::debug!("removing invalid cache entry: {}", file.path().display());

            tokio::fs::remove_file(file.path())
                .await
                .into_diagnostic()
                .wrap_err_with(|| {
                    miette!(
                        "cache contained an unexpected file ({}) and buffrs was unable to clean it up",
                        file.path().display()
                    )
                })?;
        }

        Ok(())
    }

    /// Resolve a file requirement from the cache
    pub async fn get(&self, file: FileRequirement) -> miette::Result<Option<Package>> {
        let entry: Entry = file.into();

        let file = self.path().join(entry.filename());

        let tgz = tokio::fs::read(&file)
            .await
            .into_diagnostic()
            .map(bytes::Bytes::from);

        if let Ok(tgz) = tgz {
            let pkg = Package::parse(tgz)?;

            return Ok(Some(pkg));
        }

        Ok(None)
    }

    /// Put a locked package in the cache
    pub async fn put(&self, entry: Entry, bytes: Bytes) -> miette::Result<()> {
        let file = self.path().join(entry.filename());

        tokio::fs::write(&file, bytes.as_ref())
            .await
            .into_diagnostic()
            .wrap_err(miette!(
                "failed to put package {} in the cache",
                entry.filename().to_str().unwrap()
            ))?;

        Ok(())
    }

    /// The directory in the filesystem used by this cache
    pub fn path(&self) -> &Path {
        self.0.as_path()
    }
}

/// A cache locator to store or retrieve a package
///
/// This follows the naming scheme of {package-name}-{digest-type}-{digest}.tgz
pub struct Entry(PathBuf);

impl Entry {
    /// The filename of the cache entry
    pub fn filename(&self) -> &Path {
        self.0.as_path()
    }
}

impl From<Package> for Entry {
    fn from(value: Package) -> Self {
        Self::from(&value)
    }
}

impl From<&Package> for Entry {
    fn from(value: &Package) -> Self {
        let digest = value.digest();
        Self(
            format!(
                "{}.{}.{}.tgz",
                value.name(),
                digest.algorithm(),
                hex::encode(digest.as_bytes())
            )
            .into(),
        )
    }
}

impl From<FileRequirement> for Entry {
    fn from(req: FileRequirement) -> Entry {
        Self::from(&req)
    }
}

impl From<&FileRequirement> for Entry {
    fn from(req: &FileRequirement) -> Entry {
        Self(
            format!(
                "{}.{}.{}.tgz",
                req.package,
                req.digest.algorithm(),
                hex::encode(req.digest.as_bytes())
            )
            .into(),
        )
    }
}
