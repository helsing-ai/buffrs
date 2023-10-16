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

//! # Package Storage trait and implementations.
//!
//! This module contains the [`Storage`] trait, which is used by the buffrs registry to store
//! packages in arbitrary places. Depending on the enabled features, this contains implementations
//! and layers that can be used by the registry.
//!
//! Currently, all package fetches will go through the registry. In the future, it might be
//! implemented to allow the package registry to redirect directly to a presigned URL for a storage
//! endpoint, such as a S3 bucket.

use crate::types::PackageVersion;
use bytes::Bytes;
use std::{fmt, sync::Arc};

#[cfg(feature = "storage-cache")]
mod cache;
mod filesystem;
#[cfg(feature = "storage-s3")]
mod s3;

#[cfg(feature = "storage-cache")]
pub use cache::{Cache, CacheConfig};
pub use filesystem::Filesystem;
#[cfg(feature = "storage-s3")]
pub use s3::S3;

/// Generic, shared error type.
///
/// As the underlying error type used by the implementation is not known, this error type is used
/// to allow errors to be cached when appropriate. Using an [`Arc`] here allows the error to be
/// cloned and stored, while retaining as much information as possible.
pub type SharedError = Arc<dyn std::error::Error + Send + Sync>;

/// Error putting a package into storage.
///
/// This classifies the errors produced downstream according to their semantics. The only error we
/// really care about at the moment is the `PackageMissing` case, because that one has different
/// caching semantics than other errors.
#[derive(thiserror::Error, Debug, Clone)]
pub enum StorageError {
    /// Package missing
    #[error("package missing")]
    PackageMissing(#[source] SharedError),

    /// Unknown error
    #[error(transparent)]
    Other(#[from] SharedError),
}

/// Arbitrary storage instance.
pub type AnyStorage = Arc<dyn Storage>;

/// # Storage for package sources
///
/// This trait specifies a generic storage implementation for package sources. These will store the
/// compressed tarball containing the package sources.
///
/// ## Error handling
///
/// In general, errors are always passed through and never hidden. That is why there is a
/// [`PackageMissing`][StorageError::PackageMissing] error that is returned rather than the call
/// simply returning an [`Result<Option<Bytes>>`]. This allows downstream users to inspect the
/// errors themselves if needed, and allows for more descriptive error logs.
///
/// The underlying errors are stored as a [`SharedError`], which uses an [`Arc`] to allow errors to
/// be cloned. This allows for caching errors, where it makes sense.
///
/// ## Put semantics
///
/// The semantics of the [`package_put`](Storage::package_put) call are overwrite (rather than
/// error on existing package).  That might be surprising, since packages are considered immutable
/// once published. But the justification for this is that we have a distributed system here, in
/// which the database is the leader.
///
/// In case of an error during package publishing, the transaction that adds the package to the
/// database is not committed, which could result in a package being in storage but not in the
/// database. In that case, the user will retry publishing which should succeed.
///
/// Having dirty data in a caching layer cannot happen because it could only be in the cache if it
/// ends up in the database.
///
/// The database is responsible for avoiding concurrent package publishes that would result in race
/// conditions. Additionally, checksums and signatures are used to verify package sources.
#[async_trait::async_trait]
pub trait Storage: Send + Sync + fmt::Debug {
    /// Write package to storage.
    ///
    /// In general, packages are immutable once stored. However, the semantics of this call are
    /// those of overwrite. Refer to the documentation of the trait for more context.
    async fn package_put(&self, version: &PackageVersion, data: &[u8]) -> Result<(), StorageError>;

    /// Get package from storage.
    ///
    /// If the package does not exist, this will return a [`StorageError::PackageMissing`]. This
    /// call should only succeed once the package has been successfully written.
    async fn package_get(&self, version: &PackageVersion) -> Result<Bytes, StorageError>;
}
