//! Unit tests for [`S3`].
//!
//! These test verify that the S3 storage layer is implemented correctly. Every single test
//! uses a new temporary bucket created by [`temp_s3`] to ensure that tests do not interfere
//! with each other. Every single test performs some setup using manual bucket interactions,
//! run at most one method under test, and verify the outputs and the bucket side effects.

use super::*;
use aws_credential_types::Credentials;
use aws_sdk_s3::{
    primitives::{ByteStream, SdkBody},
    types::*,
    Client,
};

use rand::{thread_rng, Rng};
use std::error::Error;

/// Generate random name for a bucket.
fn random_bucket() -> String {
    let mut rng = thread_rng();
    (0..16).map(|_| rng.gen_range('a'..='z')).collect()
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

    Client::new(&config)
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
            .client()
            .get_object()
            .bucket(storage.bucket())
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
async fn can_write_package_existing(version: PackageVersion, previous: Vec<u8>, contents: Vec<u8>) {
    with(temp_s3, |storage| async move {
        // put an object into storage manually
        storage
            .client()
            .put_object()
            .bucket(storage.bucket())
            .key(version.file_name())
            .body(ByteStream::new(SdkBody::from(previous)))
            .send()
            .await
            .unwrap();

        // overwrite it using trait
        storage.package_put(&version, &contents).await.unwrap();

        // check that it was overwritten
        let response = storage
            .client()
            .get_object()
            .bucket(storage.bucket())
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
            .client()
            .put_object()
            .bucket(storage.bucket())
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
