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

use buffrs_registry::storage::*;
use buffrs_registry::types::PackageVersion;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
pub use test_strategy::proptest;

mod filesystem;
#[cfg(feature = "storage-s3")]
mod s3;

/// Generic future used for cleanup tasks.
pub type Cleanup = Pin<Box<dyn Future<Output = ()>>>;

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
}

/// Create temporary instances of all storage providers.
async fn temp_instances() -> (Vec<AnyStorage>, Cleanup) {
    let mut storage: Vec<AnyStorage> = vec![];
    let mut cleanup: Vec<Cleanup> = vec![];

    // create filesystem instances
    create_temp_instances(&mut storage, &mut cleanup, filesystem::temp_filesystem).await;

    /*
    // create s3 instances, if enabled.
    #[cfg(feature = "storage-s3")]
    create_temp_instances(&mut storage, &mut cleanup, super::s3::tests::temp_s3).await;
    */

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
            storage.package_put(version, bytes).await.unwrap();

            let result = storage.package_get(version).await.unwrap();
            assert_eq!(result, bytes);
        }
    }

    cleanup.await;
}
