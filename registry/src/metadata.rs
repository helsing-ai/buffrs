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

//! # Metadata service traits and implementations
//!
//! This module encapsulates the metadata using the [`Metadata`], [`WriteHandle`] and
//! [`ReadHandle`] types.
//!
//! Semantically, the metadata service is a transactional store.  Crucially, this store needs to
//! always be consistent, as responses might be cached downstream.  The [`ReadHandle`] and
//! [`WriteHandle`] traits expose high-level operations that are used by the registry.
//!
//! Typically, this store is implemented using a (relational) database. By default, the registry
//! uses a Postgres database that it talks to using the `sqlx` crate.

pub mod entities;
mod postgres;
#[cfg(test)]
pub mod tests;

pub use postgres::Postgres;

use crate::types::*;
use async_trait::async_trait;
use entities::*;
use sqlx::{query, query_as};
use std::sync::Arc;
use url::Url;

/// Shared generic error type.
pub type SharedError = Arc<dyn std::error::Error + Send + Sync>;

/// Boxed generic error type.
pub type BoxError = Arc<dyn std::error::Error + Send + Sync>;

/// Shared generic metadata instance.
///
/// This type is how downstream users should consume metadata instances.
pub type AnyMetadata = Arc<dyn Metadata>;

/// Metadata trait.
///
/// This trait represents a service that stores metadata in a consistent view. The semantics of
/// this store are transactional, meaning that in-progress writes should not be visible unless they
/// are committed. There are also some important constraints of preventing race conditions when
/// publishing packages.
///
/// There may be a limited number of connections to the metadata service, in which case the calls
/// used for creating new handles may be blocking unless other handles are released.
#[async_trait]
pub trait Metadata: Send + Sync + std::fmt::Debug {
    /// Get a read handle to use for reading.
    async fn read(&self) -> Result<Box<dyn ReadHandle>, SharedError>;

    /// Get a write handle to use for writing.
    async fn write(&self) -> Result<Box<dyn WriteHandle>, SharedError>;
}

/// Error in metadata service interaction.
#[derive(thiserror::Error, Debug)]
pub enum ReadError {
    #[error("not found")]
    NotFound(#[source] SharedError),

    #[error(transparent)]
    Other(#[from] SharedError),
}

/// Handle used for reading from the metadata service.
#[async_trait]
pub trait ReadHandle: Send + Sync {
    /// Lookup a user by token.
    async fn user_info(&mut self, user: &Handle) -> Result<UserInfo, ReadError>;

    /// Check token
    async fn token_check(&mut self, token: &Token) -> Result<TokenInfo, ReadError>;

    /// Get info on token
    async fn token_info(&mut self, token: &TokenPrefix) -> Result<TokenInfo, ReadError>;

    /// Package metadata
    async fn package_metadata(&mut self, package: &str) -> Result<String, ReadError>;

    /// Package versions
    async fn package_versions(&mut self, package: &str) -> Result<Vec<String>, ReadError>;
}

/// Error in metadata service interaction.
#[derive(thiserror::Error, Debug)]
pub enum WriteError {
    #[error("not found")]
    NotFound(#[source] SharedError),

    #[error(transparent)]
    Other(#[from] SharedError),
}

/// Handle used for writing to the metadata service.
///
/// This handle also implements the [`ReadHandle`] trait, and can thus be used to read written
/// data. However, the changes made using calls in this trait are not visible from other handles
/// unless they are committed, using the [`commit()`](WriteHandle::commit) call.
#[async_trait]
pub trait WriteHandle: ReadHandle + Send + Sync {
    /// Create a user
    async fn user_create(&mut self, user: &Handle) -> Result<(), WriteError>;

    async fn user_token_create(
        &mut self,
        user: &Handle,
        token: &Token,
        permissions: &TokenPermissions,
    ) -> Result<(), WriteError>;

    async fn user_token_delete(
        &mut self,
        user: &Handle,
        token: &TokenPrefix,
    ) -> Result<(), WriteError>;

    /// Create a new package.
    async fn package_create(&mut self, package: &str) -> Result<(), WriteError>;

    /// Create a new package version.
    async fn package_version_create(
        &mut self,
        package: &str,
        version: &str,
        signature: &str,
    ) -> Result<(), WriteError>;

    /// Yank this package version.
    async fn package_version_yank(
        &mut self,
        package: &str,
        version: &str,
        signature: &str,
    ) -> Result<(), WriteError>;

    /// Increment package version download counter.
    async fn package_version_download(
        &mut self,
        package: &str,
        version: &str,
        count: u64,
    ) -> Result<(), WriteError>;

    /// Commit changes made by this handle.
    ///
    /// There are situations in which this can fail. In that case, all of the changes made by this
    /// handle are discarded.
    ///
    /// If changes are not intended to be committed, the handle may simply be dropped, which causes
    /// them to be aborted.
    async fn commit(self: Box<Self>) -> Result<(), WriteError>;
}
