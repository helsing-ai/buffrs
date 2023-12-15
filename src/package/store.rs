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

use lazy_static::lazy_static;
use miette::{bail, ensure, miette, Context, IntoDiagnostic};
use regex::{Captures, Regex};
use tokio::fs;
use walkdir::WalkDir;

use crate::{
    manifest::{Manifest, PackageManifest, MANIFEST_FILE},
    package::{Package, PackageName, PackageType},
};

lazy_static! {
    static ref PROTOBUF_IMPORT_STATEMENT_REGEX: Regex =
        Regex::new("(?m)(^import \")(.+)(\";$)").unwrap();
    static ref PROTOBUF_PACKAGE_STATEMENT_REGEX: Regex =
        regex::Regex::new("(?m)^package ([0-9A-Za-z_]+);$").unwrap();
}

/// IO abstraction layer over local `buffrs` package store
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageStore {
    root: PathBuf,
}

impl PackageStore {
    /// Path to the proto directory
    pub const PROTO_PATH: &'static str = "proto";
    /// Path to the dependency store
    pub const PROTO_VENDOR_PATH: &'static str = "proto/vendor";

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
    async fn open(path: &Path) -> miette::Result<Self> {
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

    /// Path to where the package contents are populated.
    fn populated_path(&self, manifest: &PackageManifest) -> PathBuf {
        self.proto_vendor_path().join(manifest.name.to_string())
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
            Ok(()) => {}
            Err(err) if matches!(err.kind(), std::io::ErrorKind::NotFound) => {}
            Err(_) => return Err(miette!("failed to clear {path:?} directory",)),
        }

        fs::create_dir(&path)
            .await
            .map_err(|_| miette!("failed to reinitialize {path:?} directory after cleaning"))
    }

    /// Unpacks a package into a local directory
    pub async fn unpack(&self, package: &Package) -> miette::Result<()> {
        let pkg_dir = self.locate(package.name());

        package.unpack(&pkg_dir).await?;

        tracing::debug!(
            ":: unpacked {}@{} into {}",
            package.name(),
            package.version(),
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
        manifest: &PackageManifest,
    ) -> miette::Result<crate::validation::Violations> {
        let root_path = self.proto_vendor_path();
        let source_files = self.populated_files(manifest).await;

        let mut parser = crate::validation::Validator::new(&root_path, manifest);

        for file in &source_files {
            parser.input(file);
        }

        parser.validate()
    }

    /// Packages a release from the local file system state
    pub async fn release(&self, manifest: &Manifest) -> miette::Result<Package> {
        for dependency in manifest.dependencies.iter() {
            let resolved = self.resolve(&dependency.package).await?;

            let Some(ref resolved_pkg) = resolved.package else {
                bail!("upstream package is invalid, [package] section is missing in manifest");
            };

            ensure!(
                resolved_pkg.kind != PackageType::Api,
                "depending on API packages is not allowed",
            );
        }

        let pkg_path = self.proto_path();
        let mut entries = BTreeMap::new();

        let package_name = &manifest.package.as_ref().unwrap().name;
        let compat_name = package_name.replace('-', "_");
        let compat_dir = format!("{compat_name}/");

        let parts: Vec<&str> = compat_name.split('_').collect();

        for entry in self.collect(&pkg_path, false).await {
            let mut path = entry.strip_prefix(&pkg_path).into_diagnostic()?;
            let mut contents = tokio::fs::read(&entry).await.unwrap();

            // automatic translation from legacy packages
            if parts.len() >= 3 {
                let group_name = parts[1];
                if let Ok(stripped_path) = path.strip_prefix(&compat_dir) {
                    tracing::info!("   compat: {:?} -> {:?}", path, stripped_path);
                    // NOTE: we could also work with bytes, but it is very annoying
                    // TODO: maybe refactor once ascii_chars becomes stable
                    let decoded_contents = std::str::from_utf8(&contents)
                        .into_diagnostic()
                        .wrap_err(miette!("failed to decode contents of {:?} as UTF-8", path))?;

                    // rename imports to use Buffrs-style package names (hyphens instead of undercores)
                    let renamed_imports = PROTOBUF_IMPORT_STATEMENT_REGEX.replace_all(
                        decoded_contents,
                        |caps: &Captures| {
                            format!("{}{}{}", &caps[1], caps[2].replace('_', "-"), &caps[3])
                        },
                    );

                    // change package statement
                    // find value of package statement
                    let c = PROTOBUF_PACKAGE_STATEMENT_REGEX
                        .captures(&renamed_imports)
                        .ok_or_else(|| miette!("failed to find package statement in {:?}", path))?;
                    let statement_value = c.get(1).unwrap().as_str();
                    // replace package statement
                    let renamed_accesses = renamed_imports.replace(
                        &format!("{statement_value}."),
                        &format!("{group_name}.{statement_value}."),
                    );
                    let renamed_package = renamed_accesses.replace(
                        &format!("package {statement_value};"),
                        &format!("package {group_name}.{statement_value};"),
                    );

                    contents = renamed_package.as_bytes().to_vec();
                    path = stripped_path;
                }
            }
            entries.insert(path.into(), contents.into());
        }

        let package = Package::create(manifest.clone(), entries)?;

        tracing::info!(":: packaged {}@{}", package.name(), package.version());

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
                    true
                } else {
                    !path.starts_with(self.proto_vendor_path())
                }
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

    /// Sync this stores proto files to the vendor directory
    pub async fn populate(&self, manifest: &PackageManifest) -> miette::Result<()> {
        let source_path = self.proto_path();
        let target_dir = self.proto_vendor_path().join(manifest.name.to_string());

        if tokio::fs::try_exists(&target_dir)
            .await
            .into_diagnostic()
            .wrap_err(format!(
                "failed to check whether directory {} still exists",
                target_dir.to_str().unwrap()
            ))?
        {
            tokio::fs::remove_dir_all(&target_dir)
                .await
                .into_diagnostic()
                .wrap_err(format!(
                    "failed to remove directory {} and its contents.",
                    target_dir.to_str().unwrap()
                ))?;
        }

        for entry in self.collect(&source_path, false).await {
            let file_name = entry.strip_prefix(&source_path).into_diagnostic()?;
            let target_path = target_dir.join(file_name);

            tokio::fs::create_dir_all(target_path.parent().unwrap())
                .await
                .into_diagnostic()
                .wrap_err(format!(
                    "Failed to create directory {} and its parents.",
                    target_path.parent().unwrap().to_str().unwrap()
                ))?;

            tokio::fs::copy(entry, target_path)
                .await
                .into_diagnostic()?;
        }

        Ok(())
    }

    /// Get the paths of all files under management after population
    pub async fn populated_files(&self, manifest: &PackageManifest) -> Vec<PathBuf> {
        self.collect(&self.populated_path(manifest), true).await
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
