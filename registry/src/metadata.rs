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

//! # Metadata trait and implementations
//!
//! This module encapsulates the metadata storage using the [`Pool`] and [`Metadata`] types. This
//! is used by the buffrs registry to store important data about packages that are exposed by the
//! API.

pub mod entities;
mod postgres;
#[cfg(test)]
pub mod tests;

use async_trait::async_trait;
use entities::*;
pub use postgres::Postgres;
use sqlx::{query, query_as};
use std::sync::Arc;
use url::Url;

#[derive(thiserror::Error, Debug)]
pub enum AuthError {
    #[error("token not found")]
    TokenNotFound,
    #[error("token expired")]
    TokenExpired,
    #[error("token deleted")]
    TokenDeleted,
}

pub type BoxError = Box<dyn std::error::Error + Send + Sync>;

pub type AnyMetadata = Arc<dyn Metadata>;

/// Metadata trait.
#[async_trait]
pub trait Metadata: Send + Sync + std::fmt::Debug {
    /// Get a write handle to use for writing.
    async fn write(&self) -> Result<Box<dyn WriteHandle>, BoxError>;

    /// Get a read handle to use for reading.
    async fn read(&self) -> Result<Box<dyn ReadHandle>, BoxError>;
}

/// WriteHandle interactions.
#[async_trait]
pub trait ReadHandle: Send + Sync {
    /// Lookup a user by token.
    async fn user_lookup(&mut self, user: &str) -> User;

    async fn user_info(&mut self, user: &str) -> String;

    async fn package_metadata(&mut self, package: &str) -> String;

    async fn package_version(&mut self, package: &str) -> Vec<String>;
}

/// WriteHandle interactions.
#[async_trait]
pub trait WriteHandle: ReadHandle + Send + Sync {
    /// Create a user
    async fn user_create(&mut self, user: &str) -> Result<(), ()>;

    async fn user_token_create(&mut self, user: &str, token: &str) -> Result<(), ()>;
    async fn user_token_delete(&mut self, user: &str, token: &str) -> Result<(), ()>;

    /// Create a new package.
    async fn package_create(&mut self, package: &str) -> Result<(), ()>;

    /// Create a new package version.
    async fn package_version_create(
        &mut self,
        package: &str,
        version: &str,
        signature: &str,
    ) -> Result<(), ()>;

    /// Yank this package version.
    async fn package_version_yank(
        &mut self,
        package: &str,
        version: &str,
        signature: &str,
    ) -> Result<(), ()>;

    /// Increment package version download counter.
    async fn package_version_download(
        &mut self,
        package: &str,
        version: &str,
        count: u64,
    ) -> Result<(), ()>;

    /// Commit changes
    async fn commit(self: Box<Self>) -> Result<(), ()>;
}
