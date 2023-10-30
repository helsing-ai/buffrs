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

use crate::storage::{Storage, StorageError};
use crate::types::PackageVersion;
use bytes::Bytes;
use moka::{future::Cache as MokaCache, Expiry};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

/// Cache entry for one crate lookup.
#[derive(Clone, Debug)]
enum Entry {
    /// Crate is missing.
    Missing,
    /// Crate exists,
    Data(Bytes),
}

impl Entry {
    fn weight(&self) -> usize {
        match self {
            Self::Missing => 1,
            Self::Data(bytes) => bytes.len(),
        }
    }
}

/// CacheConfiguration for storage [`Cache`].
#[derive(Clone, Copy, Debug)]
pub struct CacheConfig {
    /// Capacity of cache, in bytes.
    ///
    /// You should set this to however much memory you are willing to use for the cache. In
    /// general, the higher you set this to, the better. The default is set to 16MB.
    pub capacity: u64,

    /// Timeout for missing crate entries.
    ///
    /// Packages are essentially immutable once published, so we can cache them forever.  However,
    /// we are also caching negative lookup results (the [`Entry::Missing`]), and this is something
    /// that may change, so we do not want to cache it forever. So what we do here is we have a
    /// timeout set for those lookups that are negative. The default is to timeout missing entries
    /// after one minute.
    pub timeout_missing: Duration,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            capacity: 16 * 1024 * 1024,
            timeout_missing: Duration::from_secs(60),
        }
    }
}

impl Expiry<PackageVersion, Entry> for CacheConfig {
    fn expire_after_create(
        &self,
        key: &PackageVersion,
        value: &Entry,
        created_at: Instant,
    ) -> Option<Duration> {
        match value {
            Entry::Missing => Some(self.timeout_missing),
            Entry::Data(_) => None,
        }
    }
}

/// Storage caching layer.
///
/// This is a layer you can use to wrap an existing storage provider to add an in-memory cache
/// using the moka crate. This crate is optimized for highly concurrent, lock-free access.
#[derive(Clone, Debug)]
pub struct Cache<S: Storage> {
    storage: Arc<S>,
    cache: MokaCache<PackageVersion, Entry>,
}

impl<S: Storage> Cache<S> {
    /// Create new caching layer on top of a storage.
    ///
    /// You can use [`CacheConfig::default()`] to use defaults, which should be sane. Read the
    /// documentation on [`CacheConfig`] for more information on what can be tuned.
    pub fn new(storage: S, config: CacheConfig) -> Self {
        let cache = MokaCache::builder()
            .weigher(|_key, value: &Entry| -> u32 { value.weight().try_into().unwrap_or(u32::MAX) })
            .max_capacity(config.capacity)
            .expire_after(config)
            .build();
        let storage = Arc::new(storage);

        Self { storage, cache }
    }

    /// Get a reference to the underlying storage.
    pub fn storage(&self) -> &Arc<S> {
        &self.storage
    }

    /// Clear the cache.
    ///
    /// This will invalidate all cache entries.
    pub fn clear(&self) {
        self.cache.invalidate_all();
    }
}

#[async_trait::async_trait]
impl<S: Storage> Storage for Cache<S> {
    /// Write new package to storage.
    async fn package_put(&self, version: &PackageVersion, data: &[u8]) -> Result<(), StorageError> {
        self.storage().package_put(version, data).await
    }

    /// Get package from storage.
    async fn package_get(&self, version: &PackageVersion) -> Result<Bytes, StorageError> {
        let storage = self.storage.clone();
        let result = self
            .cache
            .try_get_with(version.clone(), async move {
                match storage.package_get(&version).await {
                    Ok(bytes) => Ok(Entry::Data(bytes)),
                    Err(StorageError::PackageMissing) => Ok(Entry::Missing),
                    Err(error) => Err(error),
                }
            })
            .await;

        match result {
            Ok(Entry::Data(bytes)) => Ok(bytes),
            Ok(Entry::Missing) => Err(StorageError::PackageMissing),
            Err(error) => Err((*error).clone()),
        }
    }
}
