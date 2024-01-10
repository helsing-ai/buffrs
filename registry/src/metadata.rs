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

use buffrs::manifest::PackageManifest;

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
///
#[derive(thiserror::Error, Debug, Clone)]
pub enum MetadataStorageError {
    /// Package missing
    #[error("package missing")]
    PackageMissing(String, String),

    /// Duplicate package
    /// Used on put_version mostly
    #[error("duplicate package")]
    PackageDuplicate(String, String),

    /// Unknown error
    #[error(transparent)]
    Other(#[from] SharedError),

    /// Unknown error
    #[error("Internal Error")]
    Internal,
}

/// [`MetadataStorage`] Instance
pub type AnyMetadataStorage = Arc<dyn MetadataStorage>;

/// Basic definition of a metadata storage
///
#[async_trait::async_trait]
pub trait MetadataStorage: Send + Sync + fmt::Debug {
    /// Gets the version
    async fn get_version(
        &self,
        package: PackageVersion,
    ) -> Result<PackageManifest, MetadataStorageError>;

    /// Puts the given package in the storage
    async fn put_version(&self, package: PackageManifest) -> Result<(), MetadataStorageError>;
}
