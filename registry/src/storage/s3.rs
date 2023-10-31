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
    primitives::{ByteStream, SdkBody},
    Client,
};
use bytes::Bytes;
use std::sync::Arc;

/// S3-backend storage.
#[derive(Clone, Debug)]
pub struct S3 {
    client: Client,
    bucket: String,
}

impl S3 {
    pub fn new(client: Client, bucket: String) -> Self {
        Self { client, bucket }
    }
}

#[async_trait::async_trait]
impl Storage for S3 {
    async fn package_put(&self, version: &PackageVersion, data: &[u8]) -> Result<(), StorageError> {
        let body = SdkBody::from(data);
        self.client
            .put_object()
            .bucket(&self.bucket)
            .key(version.file_name())
            .body(ByteStream::new(SdkBody::from(body)))
            .send()
            .await
            .map_err(|error| StorageError::Other(Arc::new(error)))?;
        Ok(())
    }

    async fn package_get(&self, version: &PackageVersion) -> Result<Bytes, StorageError> {
        let response = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(version.file_name())
            .send()
            .await
            .map_err(|error| StorageError::Other(Arc::new(error)))?;
        let data = response
            .body
            .collect()
            .await
            .map_err(|error| StorageError::Other(Arc::new(error)))?;
        Ok(data.into_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_config::ConfigLoader;
    use aws_credential_types::Credentials;
    use aws_sdk_s3::{types::*, Config};
    use rand::{distributions::Alphanumeric, thread_rng, Rng};

    fn random_bucket() -> String {
        let mut rng = thread_rng();
        (0..10).map(|_| rng.gen_range('a'..'z')).collect()
    }

    async fn test_client() -> Client {
        let credentials = Credentials::from_keys("buffrs", "password", None);
        let config = aws_config::from_env()
            .endpoint_url("http://localhost:9000")
            .region("us-east-1")
            .credentials_provider(credentials)
            .load()
            .await;
        Client::new(&config)
    }

    #[tokio::test]
    async fn can_connect() {
        let client = test_client().await;
        let bucket = random_bucket();

        client.create_bucket().bucket(&bucket).send().await.unwrap();

        client.delete_bucket().bucket(bucket).send().await.unwrap();
    }
}
