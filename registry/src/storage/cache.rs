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

//! # Storage cache layer.

use crate::storage::{SharedError, Storage, StorageError};
use crate::types::PackageVersion;
use bytes::Bytes;
use moka::{future::Cache as MokaCache, Expiry};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

/// Cached error.
///
/// This type adds context to the error it contains to communicate to users that this error may
/// have been cached.
#[derive(Debug, thiserror::Error)]
#[error("cached error")]
struct CachedError(#[from] SharedError);

/// Cache entry for one crate lookup.
#[derive(Clone, Debug)]
enum Entry {
    /// Crate is missing.
    Missing(Arc<CachedError>),
    /// Crate exists,
    Data(Bytes),
}

impl Entry {
    /// Determine the weight of this entry.
    fn weight(&self) -> usize {
        match self {
            Self::Missing(_) => 1,
            Self::Data(bytes) => bytes.len(),
        }
    }
}

/// Configuration for storage [`Cache`].
///
/// This allows you to override the behaviour of the cache. In general, you should use the
/// [`CacheConfig::default()`] implementation to create a default configuration. The value
/// that you should consider tweaking is the `capacity`, which you should set to however much
/// memory you are willing to throw at the cache.
#[derive(Clone, Copy, Debug)]
pub struct CacheConfig {
    /// Capacity of cache, in bytes.
    ///
    /// You should set this to however much memory you are willing to use for the cache. In
    /// general, the higher you set this to, the better. The default is set to 16MB.
    pub capacity: u64,

    /// Timeout for missing crate entries.
    ///
    /// Packages are immutable once published, so we can cache them forever. However,
    /// we are also caching negative lookup results, but these can change as packages are
    /// published. For that reason, negative lookups have a dedicated cache duration that should be
    /// set to a low value.
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
        _key: &PackageVersion,
        value: &Entry,
        _created_at: Instant,
    ) -> Option<Duration> {
        match value {
            Entry::Missing(_) => Some(self.timeout_missing),
            Entry::Data(_) => None,
        }
    }
}

/// Storage caching layer.
///
/// This is a layer you can use to wrap an existing storage provider to add an in-memory cache.
/// This allows you to serve commonly requested packages more efficiently.
///
/// The cache is implemented using the moka crate, which is optimized for highly concurrent,
/// lock-free access.
///
/// # Examples
///
/// Wrapping an existing storage implementation in the caching layer:
///
/// ```rust
/// # use buffrs_registry::storage::{Filesystem, Cache, CacheConfig};
/// // underlying storage implementation
/// let storage = Filesystem::new("/path/to/storage");
///
/// // wrap in caching layer with a capacity of 100MB.
/// let storage = Cache::new(storage, CacheConfig {
///     capacity: 100 * 1024 * 1024,
///     ..Default::default()
/// });
/// ```
#[derive(Clone, Debug)]
pub struct Cache<S: Storage> {
    /// Underlying storage implementation
    storage: Arc<S>,
    /// Cache used for package sources
    cache: MokaCache<PackageVersion, Entry>,
}

impl<S: Storage> Cache<S> {
    /// Create new caching layer on top of a storage.
    ///
    /// You need to create a [`CacheConfig`] to create the cache, which specifies some important
    /// metrics such as the capacity of the cache.
    /// You can use [`CacheConfig::default()`] to use defaults, which should be sane. Read the
    /// documentation on [`CacheConfig`] for more information on what can be tuned.
    pub fn new(storage: S, config: CacheConfig) -> Self {
        // we use a custom weigher to ensure that entries are weighed by their size in bytes.
        // unfortunately, the weigher only supports u32 values, so when our entry is too big (more
        // than 4GB) we will fall back to using the maximum value.
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
    async fn package_put(&self, version: &PackageVersion, data: &[u8]) -> Result<(), StorageError> {
        // we cannot cache mutable operations.
        self.storage().package_put(version, data).await
    }

    async fn package_get(&self, version: &PackageVersion) -> Result<Bytes, StorageError> {
        let storage = self.storage.clone();
        // using try_get_with here ensures that if we concurrently request the same package version
        // twice, only one lookup will be made.
        let result = self
            .cache
            .try_get_with(version.clone(), async move {
                match storage.package_get(&version).await {
                    Ok(bytes) => Ok(Entry::Data(bytes)),
                    Err(StorageError::PackageMissing(error)) => {
                        // we save the error, but wrap it in a CachedError, to communicate to
                        // the caller that this error may have been cached.
                        Ok(Entry::Missing(Arc::new(CachedError(error))))
                    }
                    Err(error) => Err(error),
                }
            })
            .await;

        // depending on what the entry is, we construct the right response.
        match result {
            Ok(Entry::Data(bytes)) => Ok(bytes),
            Ok(Entry::Missing(error)) => Err(StorageError::PackageMissing(error)),
            Err(error) => Err((*error).clone()),
        }
    }
}
