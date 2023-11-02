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

use std::{
    collections::BTreeMap,
    env::current_dir,
    path::{Path, PathBuf},
};

use miette::{ensure, miette, Context, IntoDiagnostic};
use tokio::fs;
use walkdir::WalkDir;

use crate::{
    manifest::{Manifest, MANIFEST_FILE},
    package::{Package, PackageName, PackageType},
};

/// IO abstraction layer over local `buffrs` package store
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageStore {
    root: PathBuf,
}

impl PackageStore {
    /// Path to the proto directory
    pub const PROTO_PATH: &str = "proto";
    /// Path to the dependency store
    pub const PROTO_VENDOR_PATH: &str = "proto/vendor";

    fn new(root: PathBuf) -> Self {
        Self { root }
    }

    /// Open current directory.
    pub async fn current() -> miette::Result<Self> {
        Self::open(&current_dir().into_diagnostic()?).await
    }

    /// Check if this store exists
    async fn exists(&self) -> miette::Result<bool> {
        let meta = fs::metadata(&self.proto_path()).await.into_diagnostic()?;

        Ok(meta.is_dir())
    }

    /// Open given directory.
    pub async fn open(path: &Path) -> miette::Result<Self> {
        let store = Self::new(path.into());

        if !store.exists().await? {
            miette::bail!("package store does not exist");
        }

        Ok(store)
    }

    /// Path to the `proto` directory.
    pub fn proto_path(&self) -> PathBuf {
        self.root.join(Self::PROTO_PATH)
    }

    /// Path to the vendor directory.
    pub fn proto_vendor_path(&self) -> PathBuf {
        self.root.join(Self::PROTO_VENDOR_PATH)
    }

    /// Creates the expected directory structure for `buffrs`
    pub async fn create(path: PathBuf) -> miette::Result<Self> {
        let store = PackageStore::new(path);
        let create = |dir: PathBuf| async move {
            fs::create_dir_all(&dir)
                .await
                .into_diagnostic()
                .wrap_err(miette!("failed to create {} directory", dir.display()))
        };

        create(store.proto_path()).await?;
        create(store.proto_vendor_path()).await?;

        Ok(store)
    }

    /// Clears all packages from the file system
    pub async fn clear(&self) -> miette::Result<()> {
        let path = self.proto_vendor_path();
        match fs::remove_dir_all(&path).await {
            Ok(()) => Ok(()),
            Err(err) if matches!(err.kind(), std::io::ErrorKind::NotFound) => {
                Err(miette!("directory {path:?} not found"))
            }
            Err(_) => Err(miette!("failed to clear {path:?} directory",)),
        }
    }

    /// Unpacks a package into a local directory
    pub async fn unpack(&self, package: &Package) -> miette::Result<()> {
        let pkg_dir = self.locate(&package.manifest.package.name);
        package.unpack(&pkg_dir).await?;
        tracing::debug!(
            ":: unpacked {}@{} into {}",
            package.manifest.package.name,
            package.manifest.package.version,
            pkg_dir.display()
        );

        Ok(())
    }

    /// Uninstalls a package from the local file system
    pub async fn uninstall(&self, package: &PackageName) -> miette::Result<()> {
        let pkg_dir = self.proto_vendor_path().join(&**package);

        fs::remove_dir_all(&pkg_dir)
            .await
            .into_diagnostic()
            .wrap_err(miette!("failed to uninstall package {package}"))
    }

    /// Resolves a package in the local file system
    pub async fn resolve(&self, package: &PackageName) -> miette::Result<Manifest> {
        let manifest = self.locate(package).join(MANIFEST_FILE);

        let manifest = Manifest::try_read_from(manifest)
            .await?
            .ok_or(miette!("the package store is corrupted"))?;

        Ok(manifest)
    }

    /// Validate this package
    #[cfg(feature = "validation")]
    pub async fn validate(
        &self,
        manifest: &Manifest,
    ) -> miette::Result<crate::validation::Violations> {
        let pkg_path = self.proto_path();
        let source_files = self.collect(&pkg_path, false).await;

        let mut parser = crate::validation::Validator::new(&pkg_path, &manifest.package.name);

        for file in &source_files {
            parser.input(file);
        }

        parser.validate()
    }

    /// Packages a release from the local file system state
    pub async fn release(&self, manifest: Manifest) -> miette::Result<Package> {
        ensure!(
            manifest.package.kind.is_publishable(),
            "packages with type `impl` cannot be published"
        );

        ensure!(
            !matches!(manifest.package.kind, PackageType::Lib) || manifest.dependencies.is_empty(),
            "library packages cannot have any dependencies"
        );

        for dependency in manifest.dependencies.iter() {
            let resolved = self.resolve(&dependency.package).await?;

            ensure!(
                resolved.package.kind == PackageType::Lib,
                "depending on API packages is not allowed for {} packages",
                manifest.package.kind
            );
        }

        let pkg_path = self.proto_path();
        let mut entries = BTreeMap::new();

        for entry in self.collect(&pkg_path, false).await {
            let path = entry.strip_prefix(&pkg_path).into_diagnostic()?;
            let contents = tokio::fs::read(&entry).await.unwrap();
            entries.insert(path.into(), contents.into());
        }

        let package = Package::create(manifest, entries)?;

        tracing::info!(
            ":: packaged {}@{}",
            package.manifest.package.name,
            package.manifest.package.version
        );

        Ok(package)
    }

    /// Directory for the vendored installation of a package
    pub fn locate(&self, package: &PackageName) -> PathBuf {
        self.proto_vendor_path().join(&**package)
    }

    /// Collect .proto files in a given path
    pub async fn collect(&self, path: &Path, vendored: bool) -> Vec<PathBuf> {
        let mut paths: Vec<_> = WalkDir::new(path)
            .into_iter()
            .filter_map(Result::ok)
            .map(|entry| entry.into_path())
            .filter(|path| {
                if vendored {
                    return true;
                }

                !path.starts_with(self.proto_vendor_path())
            })
            .filter(|path| {
                let ext = path.extension().map(|s| s.to_str());

                matches!(ext, Some(Some("proto")))
            })
            .collect();

        // to ensure determinism
        paths.sort();

        paths
    }
}

#[test]
fn can_get_proto_path() {
    assert_eq!(
        PackageStore::new("/tmp".into()).proto_path(),
        PathBuf::from("/tmp/proto")
    );
    assert_eq!(
        PackageStore::new("/tmp".into()).proto_vendor_path(),
        PathBuf::from("/tmp/proto/vendor")
    );
}
