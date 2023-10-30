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
use tempdir::TempDir;
use test_strategy::proptest;

async fn test_package_put(storage: &dyn Storage, version: &PackageVersion, bytes: &[u8]) {
    let result = storage.package_get(&version).await;
    assert!(matches!(result, Err(StorageError::PackageMissing)));

    storage.package_put(&version, &bytes).await.unwrap();

    let result = storage.package_get(&version).await.unwrap();
    assert_eq!(result, bytes);
}

//#[proptest(async = "tokio")]
async fn filesystem_package_put(version: PackageVersion, bytes: Vec<u8>) {
    let dir = TempDir::new("storage").unwrap();
    let storage = Filesystem::new(dir.path());
    test_package_put(&storage, &version, &bytes).await
}
