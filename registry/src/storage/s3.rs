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

use crate::storage::{Storage, StorageError};
use crate::types::PackageVersion;
use aws_sdk_s3::{
    error::SdkError,
    operation::get_object::GetObjectError,
    primitives::{ByteStream, SdkBody},
    Client,
};
use bytes::Bytes;
use std::sync::Arc;

/// # S3-backed package storage.
///
/// This storage implementation keeps packages in an S3 bucket using the `aws_sdk` crate. The
/// packages are named similar to how they are named in the filesystem.
///
/// For example, a package named `mypackage` with version `0.1.5` would be stored as
/// `mypackage_0.1.5.tar.gz` in the bucket.
#[derive(Clone, Debug)]
pub struct S3 {
    client: Client,
    bucket: String,
}

impl S3 {
    /// Create new instance given an S3 [`Client`] and a bucket name.
    pub fn new(client: Client, bucket: String) -> Self {
        Self { client, bucket }
    }

    /// Get reference to the S3 client being used.
    pub fn client(&self) -> &Client {
        &self.client
    }

    /// Get reference to the name of the bucket that this instance writes to.
    pub fn bucket(&self) -> &str {
        &self.bucket
    }
}

#[async_trait::async_trait]
impl Storage for S3 {
    async fn package_put(&self, version: &PackageVersion, data: &[u8]) -> Result<(), StorageError> {
        self.client
            .put_object()
            .bucket(&self.bucket)
            .key(version.file_name())
            .body(ByteStream::new(SdkBody::from(data)))
            .send()
            .await
            .map(|_| ())
            .map_err(|error| StorageError::Other(Arc::new(error)))
    }

    async fn package_get(&self, version: &PackageVersion) -> Result<Bytes, StorageError> {
        let response = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(version.file_name())
            .send()
            .await;

        // determine if this is a no such key error and translate into package missing
        if let Err(SdkError::ServiceError(error)) = &response {
            if let GetObjectError::NoSuchKey(error) = error.err() {
                return Err(StorageError::PackageMissing(Arc::new(error.clone())));
            }
        }

        // return other errors as-is
        let response = match response {
            Ok(response) => response,
            Err(error) => return Err(StorageError::Other(Arc::new(error))),
        };

        // collect response
        response
            .body
            .collect()
            .await
            .map_err(|error| StorageError::Other(Arc::new(error)))
            .map(|data| data.into_bytes())
    }
}
