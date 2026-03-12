// Copyright 2025 Helsing GmbH
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

//! Backward compatibility tests for legacy buffrs formats.
//!
//! Each version module uses macros from [`ensure`] to verify that lockfiles,
//! manifests, and package tars produced by that version can still be parsed.
//!
//! # Adding a new version
//!
//! 1. Create fixture data at `tests/data/legacy/v<MAJOR>_<MINOR>_<PATCH>/`
//!    with `lib/` and `api/` subdirectories (see existing fixtures).
//! 2. Add a new module below:
//!    ```ignore
//!    mod v0_13_0 {
//!        crate::ensure::parsable!(0, 13, 0);
//!        crate::ensure::installable!(0, 13, 0);
//!    }
//!    ```

mod ensure {
    /// Generates tests asserting that manifests and lockfiles from a legacy
    /// version can be parsed by the current buffrs code.
    ///
    /// Expects fixtures at:
    /// - `tests/data/legacy/v<MAJOR>_<MINOR>_<PATCH>/lib/Proto.toml`
    /// - `tests/data/legacy/v<MAJOR>_<MINOR>_<PATCH>/lib/Proto.lock`
    /// - `tests/data/legacy/v<MAJOR>_<MINOR>_<PATCH>/api/Proto.toml`
    macro_rules! parse {
        ($major:literal, $minor:literal, $patch:literal) => {
            mod parse {
                use std::str::FromStr;

                use buffrs::{
                    io::File,
                    lock::{Lockfile, PackageLockfile},
                    manifest::PackagesManifest,
                    package::{PackageName, PackageType},
                };

                pub(super) const LIB_MANIFEST: &str = include_str!(concat!(
                    "../data/legacy/v",
                    stringify!($major),
                    "_",
                    stringify!($minor),
                    "_",
                    stringify!($patch),
                    "/lib/Proto.toml"
                ));

                pub(super) const LIB_LOCKFILE: &str = include_str!(concat!(
                    "../data/legacy/v",
                    stringify!($major),
                    "_",
                    stringify!($minor),
                    "_",
                    stringify!($patch),
                    "/lib/Proto.lock"
                ));

                pub(super) const API_MANIFEST: &str = include_str!(concat!(
                    "../data/legacy/v",
                    stringify!($major),
                    "_",
                    stringify!($minor),
                    "_",
                    stringify!($patch),
                    "/api/Proto.toml"
                ));

                #[tokio::test]
                async fn lockfile() {
                    let tmp = tempfile::TempDir::new().unwrap();
                    let path = tmp.path().join("Proto.lock");
                    tokio::fs::write(&path, LIB_LOCKFILE).await.unwrap();
                    let _lock = PackageLockfile::load_from(&path).await.unwrap();
                    let _lock = Lockfile::load_from(&path).await.unwrap();
                }

                #[test]
                fn lib_manifest() {
                    let manifest = PackagesManifest::from_str(LIB_MANIFEST).unwrap();
                    let pkg = manifest.package.as_ref().unwrap();
                    assert_eq!(pkg.name, PackageName::new("legacy-lib").unwrap());
                    assert_eq!(pkg.version, semver::Version::new($major, $minor, $patch));
                    assert_eq!(pkg.kind, PackageType::Lib);
                }

                #[test]
                fn api_manifest() {
                    let raw = API_MANIFEST.replace("$REGISTRY", "http://localhost");
                    let manifest = PackagesManifest::from_str(&raw).unwrap();
                    let pkg = manifest.package.as_ref().unwrap();
                    assert_eq!(pkg.name, PackageName::new("legacy-api").unwrap());
                    assert_eq!(pkg.version, semver::Version::new($major, $minor, $patch));
                    assert_eq!(pkg.kind, PackageType::Api);

                    let deps = manifest.dependencies.as_ref().unwrap();
                    assert_eq!(deps.len(), 1);
                    assert_eq!(deps[0].package, PackageName::new("legacy-lib").unwrap());
                }
            }
        };
    }

    pub(crate) use parse;

    /// Generates tests asserting that package tars from a legacy version can
    /// be parsed and installed by the current buffrs code.
    ///
    /// Includes:
    /// - Parse tests for both lib and api tgz files
    /// - A full install test that spins up a test registry, serves the lib
    ///   package, and runs `buffrs install` for the api package
    ///
    /// Expects fixtures at:
    /// - `tests/data/legacy/v<MAJOR>_<MINOR>_<PATCH>/lib/legacy-lib-<VERSION>.tgz`
    /// - `tests/data/legacy/v<MAJOR>_<MINOR>_<PATCH>/api/legacy-api-<VERSION>.tgz`
    macro_rules! install {
        ($major:literal, $minor:literal, $patch:literal) => {
            mod install {
                use buffrs::package::{Package, PackageName};

                const LIB_TGZ: &[u8] = include_bytes!(concat!(
                    "../data/legacy/v",
                    stringify!($major),
                    "_",
                    stringify!($minor),
                    "_",
                    stringify!($patch),
                    "/lib/legacy-lib-",
                    stringify!($major),
                    ".",
                    stringify!($minor),
                    ".",
                    stringify!($patch),
                    ".tgz"
                ));

                const API_TGZ: &[u8] = include_bytes!(concat!(
                    "../data/legacy/v",
                    stringify!($major),
                    "_",
                    stringify!($minor),
                    "_",
                    stringify!($patch),
                    "/api/legacy-api-",
                    stringify!($major),
                    ".",
                    stringify!($minor),
                    ".",
                    stringify!($patch),
                    ".tgz"
                ));

                #[test]
                fn lib_tar() {
                    let tgz = bytes::Bytes::from_static(LIB_TGZ);
                    let pkg = Package::try_from(tgz).expect("should parse lib package tar");
                    assert_eq!(*pkg.name(), PackageName::new("legacy-lib").unwrap());
                    assert_eq!(*pkg.version(), semver::Version::new($major, $minor, $patch));
                }

                #[test]
                fn api_tar() {
                    let tgz = bytes::Bytes::from_static(API_TGZ);
                    let pkg = Package::try_from(tgz).expect("should parse api package tar");
                    assert_eq!(*pkg.name(), PackageName::new("legacy-api").unwrap());
                    assert_eq!(*pkg.version(), semver::Version::new($major, $minor, $patch));
                }

                #[test]
                fn full() {
                    crate::with_test_registry(|url| {
                        // Upload legacy-lib to the test registry.
                        // We're inside with_test_registry's multi-threaded tokio
                        // runtime, so use block_in_place to run async reqwest.
                        tokio::task::block_in_place(|| {
                            tokio::runtime::Handle::current().block_on(async {
                                reqwest::Client::new()
                                    .put(format!(
                                        "{url}/legacy/legacy-lib/legacy-lib-{major}.{minor}.{patch}.tgz",
                                        major = $major, minor = $minor, patch = $patch,
                                    ))
                                    .body(LIB_TGZ.to_vec())
                                    .send()
                                    .await
                                    .unwrap();
                            });
                        });

                        let tmp = tempfile::TempDir::new().unwrap();
                        let root = tmp.path();

                        let manifest = super::parse::API_MANIFEST.replace("$REGISTRY", url);
                        std::fs::write(root.join("Proto.toml"), &manifest).unwrap();

                        // Create a BUFFRS_HOME so credentials loading returns defaults.
                        let buffrs_home = root.join("buffrs-home");
                        std::fs::create_dir_all(&buffrs_home).unwrap();

                        // Run `buffrs install`
                        assert_cmd::Command::cargo_bin("buffrs")
                            .unwrap()
                            .env("BUFFRS_HOME", &buffrs_home)
                            .arg("install")
                            .current_dir(root)
                            .assert()
                            .success();

                        // Verify legacy-lib was unpacked into proto/vendor/
                        assert!(
                            root.join("proto/vendor/legacy-lib/Proto.toml").exists(),
                            "legacy-lib should be unpacked to proto/vendor/legacy-lib/"
                        );

                        // Verify a lockfile was created
                        assert!(
                            root.join("Proto.lock").exists(),
                            "Proto.lock should be created after install"
                        );
                    });
                }
            }
        };
    }

    pub(crate) use install;
}

mod v0_11_0 {
    super::ensure::parse!(0, 11, 0);
    super::ensure::install!(0, 11, 0);
}

mod v0_12_0 {
    super::ensure::parse!(0, 12, 0);
    super::ensure::install!(0, 12, 0);
}

mod v0_12_1 {
    super::ensure::parse!(0, 12, 1);
    super::ensure::install!(0, 12, 1);
}

mod v0_12_2 {
    super::ensure::parse!(0, 12, 2);
    super::ensure::install!(0, 12, 2);
}
