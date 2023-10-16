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

use bytes::Bytes;
use miette::{IntoDiagnostic, Result};
use std::sync::Arc;

mod filesystem;
#[cfg(feature = "storage-cache")]
mod cache;

#[derive(thiserror::Error, Debug)]
pub enum PackagePutError {
    #[error("package exists")]
    PackageExists,

    #[error(transparent)]
    Other(#[from] Box<dyn std::error::Error>),
}

#[derive(thiserror::Error, Debug)]
pub enum PackageGetError {
    #[error("package missing")]
    PackageMissing,

    #[error(transparent)]
    Other(#[from] Box<dyn std::error::Error>),
}

/// Storage for package sources
#[async_trait::async_trait]
pub trait Storage {
    /// Write new package to storage.
    async fn package_put(&self, package: &str, version: &str, data: &[u8]) -> Result<()>;

    /// Get package from storage.
    async fn package_get(&self, package: &str, version: &str) -> Result<Bytes>;
}

pub type GenericStorage = Arc<dyn Storage>;

