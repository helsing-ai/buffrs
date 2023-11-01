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
        match &response {
            Err(SdkError::ServiceError(error)) => match error.err() {
                GetObjectError::NoSuchKey(error) => {
                    return Err(StorageError::PackageMissing(Arc::new(error.clone())));
                }
                _ => {}
            },
            _ => {}
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

#[cfg(test)]
pub mod tests {
    //! Unit tests for [`S3`].
    //!
    //! These test verify that the S3 storage layer is implemented correctly. Every single test
    //! uses a new temporary bucket created by [`temp_s3`] to ensure that tests do not interfere
    //! with each other. Every single test performs some setup using manual bucket interactions,
    //! run at most one method under test, and verify the outputs and the bucket side effects.

    use super::*;
    use crate::storage::tests::{with, Cleanup};
    use aws_config::ConfigLoader;
    use aws_credential_types::Credentials;
    use aws_sdk_s3::{types::*, Config};
    use rand::{distributions::Alphanumeric, thread_rng, Rng};
    use std::error::Error;
    use test_strategy::proptest;
    use tokio::sync::OnceCell;

    /// Generate random name for a bucket.
    fn random_bucket() -> String {
        let mut rng = thread_rng();
        (0..10).map(|_| rng.gen_range('a'..'z')).collect()
    }

    /// Generate a client with test credentials.
    async fn minio_client() -> Client {
        let credentials = Credentials::from_keys("buffrs", "password", None);
        let config = aws_config::from_env()
            .endpoint_url("http://localhost:9000")
            .region("us-east-1")
            .credentials_provider(credentials)
            .load()
            .await;
        let client = Client::new(&config);
        client
    }

    /// Delete bucket.
    async fn delete_bucket(client: Client, bucket: String) {
        let objects = client
            .list_objects_v2()
            .bucket(&bucket)
            .send()
            .await
            .unwrap();

        let mut delete_objects: Vec<ObjectIdentifier> = vec![];
        for obj in objects.contents().iter().flat_map(|i| i.iter()) {
            let obj_id = ObjectIdentifier::builder()
                .set_key(Some(obj.key().unwrap().to_string()))
                .build();
            delete_objects.push(obj_id);
        }

        if !delete_objects.is_empty() {
            client
                .delete_objects()
                .bucket(&bucket)
                .delete(Delete::builder().set_objects(Some(delete_objects)).build())
                .send()
                .await
                .unwrap();
        }

        client.delete_bucket().bucket(bucket).send().await.unwrap();
    }

    /// Create test client for S3.
    pub async fn temp_s3() -> (S3, Cleanup) {
        let client = minio_client().await;
        let bucket = random_bucket();
        client.create_bucket().bucket(&bucket).send().await.unwrap();
        let s3 = S3::new(client.clone(), bucket.clone());
        (s3, Box::pin(delete_bucket(client, bucket)))
    }

    #[proptest(async = "tokio")]
    async fn can_write_package(version: PackageVersion, contents: Vec<u8>) {
        with(temp_s3, |storage| async move {
            // write package using trait
            storage.package_put(&version, &contents).await.unwrap();

            // verify manually that it is there.
            let response = storage
                .client
                .get_object()
                .bucket(&storage.bucket)
                .key(version.file_name())
                .send()
                .await
                .unwrap();
            let data = response.body.collect().await.unwrap().into_bytes();
            assert_eq!(contents, data);
        })
        .await;
    }

    #[proptest(async = "tokio")]
    async fn can_write_package_existing(
        version: PackageVersion,
        previous: Vec<u8>,
        contents: Vec<u8>,
    ) {
        with(temp_s3, |storage| async move {
            // put an object into storage manually
            storage
                .client
                .put_object()
                .bucket(&storage.bucket)
                .key(version.file_name())
                .body(ByteStream::new(SdkBody::from(previous)))
                .send()
                .await
                .unwrap();

            // overwrite it using trait
            storage.package_put(&version, &contents).await.unwrap();

            // check that it was overwritten
            let response = storage
                .client
                .get_object()
                .bucket(&storage.bucket)
                .key(version.file_name())
                .send()
                .await
                .unwrap();
            let data = response.body.collect().await.unwrap().into_bytes();
            assert_eq!(contents, data);
        })
        .await;
    }

    #[proptest(async = "tokio")]
    async fn cannot_read_package_missing(version: PackageVersion) {
        with(temp_s3, |storage| async move {
            // read a non-existing package
            let error = storage.package_get(&version).await.err().unwrap();

            // ensure we get the right error and a cause
            assert!(matches!(error, StorageError::PackageMissing(_)));
            let error = error.source().unwrap();
            assert_eq!(
                error.to_string(),
                "NoSuchKey: The specified key does not exist."
            );
        })
        .await;
    }

    #[proptest(async = "tokio")]
    async fn can_read_package(version: PackageVersion, contents: Vec<u8>) {
        with(temp_s3, |storage| async move {
            // put an object into storage manually
            storage
                .client
                .put_object()
                .bucket(&storage.bucket)
                .key(version.file_name())
                .body(ByteStream::new(SdkBody::from(&contents[..])))
                .send()
                .await
                .unwrap();

            // read a package using trait
            let found = storage.package_get(&version).await.unwrap();

            // verify it was what we had written
            assert_eq!(&found[..], &contents);
        })
        .await;
    }
}
