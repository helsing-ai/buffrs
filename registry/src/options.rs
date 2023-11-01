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

use buffrs_registry::storage::*;
use clap::{Parser, ValueEnum};
use eyre::Result;
use std::{net::SocketAddr, path::PathBuf, sync::Arc, time::Duration};
use url::Url;

#[derive(Parser, Clone, Debug)]
pub struct Options {
    /// Address to listen to for incoming connections.
    #[clap(long, short, env, default_value = "0.0.0.0:4367")]
    pub listen: SocketAddr,

    /// URL of Postgres database to connect to.
    #[clap(long, short, env)]
    #[cfg_attr(dev, clap(default_value = "postgres://buffrs:buffrs@localhost"))]
    pub database: Url,

    #[clap(flatten)]
    pub storage: StorageOptions,
}

#[cfg(feature = "storage-s3")]
const DEFAULT_STORAGE: &'static str = "s3";
#[cfg(not(feature = "storage-s3"))]
const DEFAULT_STORAGE: &'static str = "filesystem";

/// Options for storage.
#[derive(Parser, Clone, Debug)]
pub struct StorageOptions {
    /// Which storage backend to use.
    #[clap(long, default_value = DEFAULT_STORAGE)]
    pub storage: StorageKind,

    #[clap(flatten)]
    pub filesystem: FilesystemStorageOptions,

    #[clap(flatten)]
    #[cfg(feature = "storage-s3")]
    pub s3: S3StorageOptions,

    #[clap(flatten)]
    #[cfg(feature = "storage-cache")]
    pub cache: StorageCacheOptions,
}

/// Options for storage cache
#[derive(Parser, Clone, Debug)]
#[cfg(feature = "storage-cache")]
pub struct StorageCacheOptions {
    /// Enables storage cache with the specified capacity.
    #[clap(long, env)]
    pub storage_cache: bool,

    /// Storage cache capacity, in bytes.
    #[clap(long, requires("storage_cache"), env, default_value = "16000000")]
    pub storage_cache_capacity: u64,

    /// Timeout for package missing entries in the cache.
    #[clap(long, requires("storage_cache"), env, default_value = "60")]
    pub storage_cache_missing_timeout: u64,
}

#[cfg(feature = "storage-cache")]
impl StorageCacheOptions {
    fn maybe_cache<S: Storage + 'static>(&self, storage: S) -> Arc<dyn Storage> {
        if self.storage_cache {
            let config = CacheConfig {
                capacity: self.storage_cache_capacity,
                timeout_missing: Duration::from_secs(self.storage_cache_missing_timeout),
                ..Default::default()
            };
            let cache = Cache::new(storage, config);
            Arc::new(cache)
        } else {
            Arc::new(storage)
        }
    }
}

#[derive(Parser, Clone, Debug)]
pub struct FilesystemStorageOptions {
    /// Path to store packages at.
    #[clap(long, required_if_eq("storage", "filesystem"))]
    pub filesystem_storage: Option<PathBuf>,
}

#[derive(Parser, Clone, Debug)]
pub struct S3StorageOptions {}

/// Kind of storage to use.
#[derive(ValueEnum, Clone, Debug)]
pub enum StorageKind {
    Filesystem,
    #[cfg(feature = "storage-s3")]
    S3,
}

impl FilesystemStorageOptions {
    async fn build(&self) -> Result<Filesystem> {
        Ok(Filesystem::new(self.filesystem_storage.clone().unwrap()))
    }
}

#[cfg(feature = "storage-s3")]
impl S3StorageOptions {
    async fn build(&self) -> Result<S3> {
        todo!()
    }
}

impl StorageOptions {
    fn maybe_cache<S: Storage + 'static>(&self, storage: S) -> Arc<dyn Storage> {
        #[cfg(feature = "storage-cache")]
        return self.cache.maybe_cache(storage);
        #[cfg(not(feature = "storage-cache"))]
        Arc::new(storage)
    }

    pub async fn build(&self) -> Result<Arc<dyn Storage>> {
        let storage = match self.storage {
            StorageKind::Filesystem => self.maybe_cache(self.filesystem.build().await?),
            #[cfg(feature = "storage-s3")]
            StorageKind::S3 => self.maybe_cache(self.s3.build().await?),
        };

        Ok(storage)
    }
}
