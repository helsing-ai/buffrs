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

use crate::types::PackageVersion;
use bytes::Bytes;
use std::{fmt, sync::Arc};

#[cfg(feature = "storage-cache")]
mod cache;
mod filesystem;
#[cfg(test)]
mod tests;

#[cfg(feature = "storage-cache")]
pub use cache::{Cache, CacheConfig};
pub use filesystem::Filesystem;

/// Error putting a package into storage.
#[derive(thiserror::Error, Debug, Clone)]
pub enum StorageError {
    #[error("package exists")]
    PackageExists,

    #[error("package missing")]
    PackageMissing,

    #[error(transparent)]
    Other(#[from] Arc<dyn std::error::Error + Send + Sync>),
}

/// Storage for package sources
///
/// Package sources are immutable once written, which allows us to do some simple caching of data.
#[async_trait::async_trait]
pub trait Storage: Send + Sync + fmt::Debug {
    /// Write new package to storage.
    async fn package_put(&self, version: &PackageVersion, data: &[u8]) -> Result<(), StorageError>;

    /// Get package from storage.
    async fn package_get(&self, version: &PackageVersion) -> Result<Bytes, StorageError>;
}
