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

//! # Metadata Storage trait and implementations.
//!
//! [`MetadataStorage`]

/// memory provider
pub mod memory;
/// PostgreSQL storage
pub mod postgresql;

use buffrs::manifest::PackageManifest;
use buffrs::package::PackageName;
use semver::VersionReq;

use std::fmt;
use std::sync::Arc;

use crate::types::PackageVersion;

/// Generic, shared error type.
///
/// As the underlying error type used by the implementation is not known, this error type is used
/// to allow errors to be cached when appropriate. Using an [`Arc`] here allows the error to be
/// cloned and stored, while retaining as much information as possible.
pub type SharedError = Arc<dyn std::error::Error + Send + Sync>;

/// Packages errors
#[derive(thiserror::Error, Debug, Clone)]
pub enum MetadataStorageError {
    /// Package missing
    #[error("package missing")]
    PackageMissing(String, Option<String>),

    /// Duplicate package
    /// Used on put_version mostly
    #[error("duplicate package")]
    PackageDuplicate(String, String),

    /// Unknown error
    #[error(transparent)]
    Other(#[from] SharedError),

    /// Internal stuff error
    #[error("Internal Error")]
    Internal,
}

/// Fetching from metadata Storage
#[async_trait::async_trait]
pub trait TryFetch<Executor> {
    /// Try fetching a given package version
    /// Returns [`MetadataStorageError`] if it fails
    async fn try_fetch(
        version: PackageVersion,
        e: &Executor,
    ) -> Result<PackageManifest, MetadataStorageError>;
}

/// Fetching data storage
#[async_trait::async_trait]
pub trait FetchMatching<Executor> {
    /// Try fetching a given package version
    /// Returns [`MetadataStorageError`] if it fails
    async fn fetch_matching(
        package: PackageName,
        req: VersionReq,
        e: &Executor,
    ) -> Result<Vec<PackageManifest>, MetadataStorageError>;
}

/// Publishing to Metadata Storage
#[async_trait::async_trait]
pub trait Publish<Executor> {
    /// Publishes the given package
    async fn publish(package: PackageManifest, e: &Executor) -> Result<(), MetadataStorageError>;
}
