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

//! # Database
//!
//! This module encapsulates the connection the database. Outside of this module, `sqlx` should not
//! be used directly. Instead, the necessary types should be exported from this module.

mod entities;
mod postgres;
#[cfg(all(test, feature = "test-database"))]
mod tests;

use async_trait::async_trait;
pub use entities::*;
pub use postgres::*;
use sqlx::{query, query_as};
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

#[async_trait]
pub trait Pool: Send + Sync {
    async fn get(&self) -> Result<Box<dyn Database>, ()>;
    async fn begin(&self) -> Result<Box<dyn Transaction>, ()>;
}

/// Database interactions.
#[async_trait]
pub trait Database: Send + Sync {
    /// Lookup a user by token.
    async fn user_lookup(&mut self, user: &str) -> User;

    /// Create a user
    async fn user_create(&mut self, user: &str) -> Result<(), ()>;

    /// Create a token for a user
    async fn user_cert_create(&mut self, user: &str, token: &str) -> Result<(), ()>;

    /// List certificates for user
    async fn user_cert_list(&mut self, user: &str) -> Result<Vec<String>, ()>;

    /// Delete a token for a user
    async fn user_cert_delete(&mut self, user: &str, token: &str) -> Result<(), ()>;

    /// Attempt to authenticate a user by token.
    async fn user_token_auth(&mut self, token: &str) -> Result<(), ()>;

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
}

/// Database transaction.
#[async_trait]
pub trait Transaction: Database + Send + Sync {
    async fn commit(self) -> Result<(), ()>;
}
