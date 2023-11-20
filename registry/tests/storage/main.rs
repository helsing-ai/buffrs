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

use buffrs_registry::{storage::*, types::PackageVersion};
use proptest::prop_compose;
use std::{future::Future, pin::Pin, sync::Arc};
use test_strategy::{proptest, Arbitrary};

mod filesystem;
#[cfg(feature = "storage-s3")]
mod s3;

/// Generic future used for cleanup tasks.
type Cleanup = Pin<Box<dyn Future<Output = ()>>>;

/// Run a closure with a temporary instance and run cleanup afterwards.
async fn with<
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

    // create s3 instances, if enabled.
    #[cfg(feature = "storage-s3")]
    create_temp_instances(&mut storage, &mut cleanup, s3::temp_s3).await;

    let cleanup = Box::pin(async move {
        for c in cleanup.into_iter() {
            c.await;
        }
    });

    (storage, cleanup)
}

use buffrs::package::PackageName;
use semver::{BuildMetadata, Prerelease, Version};

prop_compose! {
    fn package_name()(name in "[a-z][a-z0-9-]{0,127}") -> PackageName {
        name.try_into().unwrap()
    }
}

prop_compose! {
    fn semver_version()(major: u64, minor: u64, patch: u64) -> Version {
        Version {
            minor,
            major,
            patch,
            pre: Prerelease::EMPTY,
            build: BuildMetadata::EMPTY,
        }
    }
}

prop_compose! {
    fn package_version()(
        package in package_name(),
        version in semver_version()
    ) -> PackageVersion {
        PackageVersion {
            package,
            version
        }
    }
}

#[proptest(async = "tokio", cases = 10)]
async fn can_package_put(#[strategy(package_version())] version: PackageVersion, bytes: Vec<u8>) {
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

#[derive(Arbitrary, Debug)]
struct PackageContents {
    #[strategy(package_version())]
    version: PackageVersion,
    bytes: Vec<u8>,
}

#[proptest(async = "tokio", cases = 10)]
async fn can_package_put_many(packages: Vec<PackageContents>) {
    let (instances, cleanup) = temp_instances().await;

    for storage in instances {
        println!("Testing {storage:?}");

        for package in &packages {
            storage
                .package_put(&package.version, &package.bytes)
                .await
                .unwrap();

            let result = storage.package_get(&package.version).await.unwrap();
            assert_eq!(result, package.bytes);
        }
    }

    cleanup.await;
}
