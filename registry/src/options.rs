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

use buffrs_registry::metadata::postgresql::PgsqlMetadataStorage;
use buffrs_registry::{context::Context, storage::*};
use clap::{Parser, ValueEnum};
use eyre::Result;
use std::{net::SocketAddr, path::PathBuf, sync::Arc};

#[derive(Parser, Clone, Debug)]
pub struct Options {
    /// Address to listen to for incoming connections.
    #[clap(long, short, env, default_value = "0.0.0.0:4367")]
    pub listen: SocketAddr,

    /// Storage related options.
    #[clap(flatten)]
    pub storage: StorageOptions,

    #[clap(flatten)]
    pub metadata: PgsqlStorageOptions,
}

impl Options {
    pub async fn build(&self) -> Result<Context> {
        let storage = self.storage.build().await?;

        let pg_options = PgsqlStorageOptions::parse();
        let pgsql = pg_options.build().await?;
        let metadata = Arc::new(pgsql);

        Ok(Context::new(storage, metadata, self.listen))
    }
}

#[cfg(feature = "storage-s3")]
const DEFAULT_STORAGE: &str = "s3";
#[cfg(not(feature = "storage-s3"))]
const DEFAULT_STORAGE: &str = "filesystem";

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
    fn wrap<S: Storage + 'static>(&self, storage: S) -> Arc<dyn Storage> {
        Arc::new(storage)
    }

    pub async fn build(&self) -> Result<Arc<dyn Storage>> {
        let storage = match self.storage {
            StorageKind::Filesystem => self.wrap(self.filesystem.build().await?),
            #[cfg(feature = "storage-s3")]
            StorageKind::S3 => self.wrap(self.s3.build().await?),
        };

        Ok(storage)
    }
}

/// PgSQL storage options
#[derive(Parser, Clone, Debug)]
pub struct PgsqlStorageOptions {
    /// PostgreSQL Connection string
    #[clap(
        long,
        short,
        env,
        default_value = "postgres://buffrs:buffrs@127.0.0.1/buffrs"
    )]
    pub connection_string: String,

    /// Connection pool size
    #[clap(long, short, env, default_value = "5")]
    pub max_connections: u32,
}

impl PgsqlStorageOptions {
    async fn build(&self) -> Result<PgsqlMetadataStorage> {
        Ok(PgsqlMetadataStorage::connect(&self.connection_string, self.max_connections).await?)
    }
}
