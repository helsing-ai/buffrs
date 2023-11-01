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

use super::*;
use std::future::Future;
use std::pin::Pin;
use std::time::Duration;
use tempdir::TempDir;
use test_strategy::proptest;

/// Generic future used for cleanup tasks.
pub type Cleanup = Pin<Box<dyn std::future::Future<Output = ()>>>;

/// Cache config used for tests.
///
/// This uses default values, but crucially sets the `timeout_missing` to zero. This is needed
/// because we do not want the tests to have to wait for entries to expire.
#[cfg(feature = "storage-cache")]
const TEST_CACHE_CONFIG: CacheConfig = CacheConfig {
    timeout_missing: Duration::from_secs(0),
    capacity: 16 * 1024 * 1024,
};

/// Run a closure with a temporary instance and run cleanup afterwards.
pub async fn with<
    S: Storage,
    O1: Future<Output = (S, Cleanup)>,
    F1: Fn() -> O1,
    O2: Future<Output = ()>,
    F2: FnOnce(S) -> O2,
>(
    function: F1,
    closure: F2,
) {
    let (storage, cleanup) = function().await;
    closure(storage).await;
    cleanup.await;
}

/// Create temporary instances of a storage backend. If the cache `feature` is enabled, this will
/// create an additional cached instance.
async fn create_temp_instances<
    S: Storage + 'static,
    O: Future<Output = (S, Cleanup)>,
    F: Fn() -> O,
>(
    storages: &mut Vec<AnyStorage>,
    cleanups: &mut Vec<Cleanup>,
    function: F,
) {
    // create instance
    let (storage, cleanup) = function().await;
    storages.push(Arc::new(storage));
    cleanups.push(cleanup);

    // create cached instance, if feature is enabled.
    #[cfg(feature = "storage-cache")]
    {
        let (storage, cleanup) = function().await;
        storages.push(Arc::new(Cache::new(storage, TEST_CACHE_CONFIG)));
        cleanups.push(cleanup);
    }
}

/// Create temporary instances of all storage providers.
async fn temp_instances() -> (Vec<AnyStorage>, Cleanup) {
    let mut storage: Vec<AnyStorage> = vec![];
    let mut cleanup: Vec<Cleanup> = vec![];

    // create filesystem instances
    create_temp_instances(
        &mut storage,
        &mut cleanup,
        super::filesystem::tests::temp_filesystem,
    )
    .await;

    // create s3 instances, if enabled.
    #[cfg(feature = "storage-s3")]
    create_temp_instances(&mut storage, &mut cleanup, super::s3::tests::temp_s3).await;

    let cleanup = Box::pin(async move {
        for c in cleanup.into_iter() {
            c.await;
        }
    });

    (storage, cleanup)
}

#[proptest(async = "tokio", cases = 10)]
async fn can_package_put(version: PackageVersion, bytes: Vec<u8>) {
    let (instances, cleanup) = temp_instances().await;

    for storage in instances {
        println!("Testing {storage:?}");

        let result = storage.package_get(&version).await;
        assert!(matches!(result, Err(StorageError::PackageMissing(_))));

        storage.package_put(&version, &bytes).await.unwrap();

        let result = storage.package_get(&version).await.unwrap();
        assert_eq!(result, bytes);
    }

    cleanup.await;
}

#[proptest(async = "tokio", cases = 10)]
async fn can_package_put_many(packages: Vec<(PackageVersion, Vec<u8>)>) {
    let (instances, cleanup) = temp_instances().await;

    for storage in instances {
        println!("Testing {storage:?}");

        for (version, bytes) in &packages {
            storage.package_put(&version, &bytes).await.unwrap();

            let result = storage.package_get(&version).await.unwrap();
            assert_eq!(result, bytes);
        }
    }

    cleanup.await;
}
