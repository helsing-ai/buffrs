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

use bytes::Bytes;
use miette::{Context, IntoDiagnostic, miette};
use tokio::fs;
use walkdir::WalkDir;

use crate::{
    manifest::{MANIFEST_FILE, Manifest, PackageManifest},
    package::{Package, PackageName},
};

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

    /// Opens a package store for the current working directory
    ///
    /// Creates the necessary directory structure (`proto/` and `proto/vendor/`)
    /// if it doesn't already exist.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The current directory cannot be determined
    /// - Directory creation fails due to permission issues
    pub async fn current() -> miette::Result<Self> {
        Self::open(&current_dir().into_diagnostic()?).await
    }

    /// Returns the absolute path to the `proto` directory
    ///
    /// This directory contains the source `.proto` files for the current package.
    pub fn proto_path(&self) -> PathBuf {
        self.root.join(Self::PROTO_PATH)
    }

    /// Returns the absolute path to the vendor directory
    ///
    /// This directory (`proto/vendor/`) contains installed dependencies.
    /// Each dependency is placed in a subdirectory named after the package.
    pub fn proto_vendor_path(&self) -> PathBuf {
        self.root.join(Self::PROTO_VENDOR_PATH)
    }

    /// Path to where the package contents are populated.
    fn populated_path(&self, manifest: &PackageManifest) -> PathBuf {
        self.proto_vendor_path().join(manifest.name.to_string())
    }

    /// Creates the expected directory structure for `buffrs` at the given path
    ///
    /// Initializes a package store at the specified root directory by creating:
    /// - `proto/` - For source protobuf files
    /// - `proto/vendor/` - For installed dependencies
    ///
    /// # Arguments
    ///
    /// * `path` - The root directory where the package store should be created
    ///
    /// # Errors
    ///
    /// Returns an error if directory creation fails (e.g., due to permission issues)
    pub async fn open(path: impl AsRef<Path>) -> miette::Result<Self> {
        let store = PackageStore::new(path.as_ref().to_path_buf());
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

    /// Clears all installed packages from the vendor directory
    ///
    /// Removes the entire `proto/vendor/` directory and recreates it empty.
    /// This is typically called before reinstalling dependencies to ensure a clean state.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The vendor directory cannot be removed (ignores if already missing)
    /// - The directory cannot be recreated after removal
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

    /// Unpacks a package tarball into the vendor directory
    ///
    /// Extracts the contents of a package (`.tgz` tarball) into `proto/vendor/<package_name>/`.
    /// This makes the package's protobuf files available for use as a dependency.
    ///
    /// # Arguments
    ///
    /// * `package` - The package to unpack (contains the tarball data)
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The tarball extraction fails
    /// - The target directory cannot be created
    /// - File permissions prevent writing
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

    /// Uninstalls a package from the vendor directory
    ///
    /// Removes the package directory `proto/vendor/<package_name>/` and all its contents.
    ///
    /// # Arguments
    ///
    /// * `package` - The name of the package to uninstall
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The package directory doesn't exist
    /// - Directory removal fails due to permissions or locked files
    pub async fn uninstall(&self, package: &PackageName) -> miette::Result<()> {
        let pkg_dir = self.proto_vendor_path().join(&**package);

        fs::remove_dir_all(&pkg_dir)
            .await
            .into_diagnostic()
            .wrap_err(miette!("failed to uninstall package {package}"))
    }

    /// Resolves a package manifest from the local vendor directory
    ///
    /// Looks up the package in `proto/vendor/<package_name>/Proto.toml`.
    /// This expects the package to already be installed/unpacked in the vendor directory.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The package directory doesn't exist in `proto/vendor/`
    /// - The `Proto.toml` file is missing or corruptedDo
    /// - The manifest cannot be deserialized
    pub async fn resolve(&self, package: &PackageName) -> miette::Result<Manifest> {
        let manifest = self.locate(package).join(MANIFEST_FILE);

        let manifest = Manifest::try_read_from(&manifest).await.wrap_err({
            miette!(
                "the package store is corrupted: `{}` is not present",
                manifest.display()
            )
        })?;

        Ok(manifest)
    }

    /// Validates the protobuf files in a package against buffrs rules
    ///
    /// Runs validation rules on all `.proto` files in the package to ensure they
    /// conform to buffrs conventions (naming, package structure, etc.).
    ///
    /// # Arguments
    ///
    /// * `manifest` - The package manifest describing which package to validate
    ///
    /// # Returns
    ///
    /// Returns a collection of validation violations, if any were found.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The package files cannot be read
    /// - The protobuf parser fails
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
    ///
    /// Creates an in-memory tarball (`.tgz`) containing all `.proto` files from the `proto/` directory.
    /// This collects all protobuf definitions and packages them for distribution or installation.
    ///
    /// # Arguments
    ///
    /// * `manifest` - The manifest describing the package being released
    /// * `preserve_mtime` - If `true`, preserves modification times of files in the tarball
    ///
    /// # Process
    ///
    /// 1. Collects all `.proto` files from the `proto/` directory (excluding `proto/vendor/`)
    /// 2. Creates a compressed tarball in memory
    /// 3. Returns a `Package` ready for publishing or installation
    ///
    /// # Note
    ///
    /// Dependency validation (e.g., checking for API package dependencies) is performed
    /// in the resolver during dependency graph construction, not here.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Files cannot be read from the `proto/` directory
    /// - The tarball creation fails
    pub async fn release(
        &self,
        manifest: &Manifest,
        preserve_mtime: bool,
    ) -> miette::Result<Package> {
        let pkg_path = self.proto_path();
        let mut entries = BTreeMap::new();

        for entry in self.collect(&pkg_path, false).await {
            let path = entry.strip_prefix(&pkg_path).into_diagnostic()?;
            let contents = tokio::fs::read(&entry).await.unwrap();

            entries.insert(
                path.into(),
                Entry {
                    contents: contents.into(),
                    metadata: tokio::fs::metadata(&entry).await.ok(),
                },
            );
        }

        let package = Package::create(manifest.clone(), entries, preserve_mtime)?;

        tracing::info!(":: packaged {}@{}", package.name(), package.version());

        Ok(package)
    }

    /// Returns the installation directory path for a package
    ///
    /// Returns the path where a package is (or will be) installed in the vendor directory.
    /// The path format is `proto/vendor/<package_name>/`.
    ///
    /// # Arguments
    ///
    /// * `package` - The package name to locate
    ///
    /// # Note
    ///
    /// This method does not check if the package actually exists at this location.
    pub fn locate(&self, package: &PackageName) -> PathBuf {
        self.proto_vendor_path().join(&**package)
    }

    /// Collects all `.proto` files in a given directory
    ///
    /// Recursively walks the directory tree and collects all files with the `.proto` extension.
    /// Results are sorted for deterministic output.
    ///
    /// # Arguments
    ///
    /// * `path` - The root directory to search
    /// * `vendored` - If `false`, excludes files from `proto/vendor/` (for collecting source files only)
    ///                If `true`, includes all `.proto` files regardless of location
    ///
    /// # Returns
    ///
    /// A sorted vector of absolute paths to all `.proto` files found.
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

    /// Copies the package's source `.proto` files to the vendor directory
    ///
    /// Synchronizes files from `proto/` to `proto/vendor/<package_name>/`, making
    /// the package available as a self-dependency. This is used when a package needs
    /// to reference its own protobuf files as if it were a dependency.
    ///
    /// # Process
    ///
    /// 1. Removes the existing target directory if present
    /// 2. Collects all `.proto` files from `proto/` (excluding `proto/vendor/`)
    /// 3. Copies each file to `proto/vendor/<package_name>/` preserving directory structure
    ///
    /// # Arguments
    ///
    /// * `manifest` - The package manifest containing the package name
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Directory creation or removal fails
    /// - Files cannot be copied (permissions, disk space, etc.)
    /// - Source files cannot be read
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

    /// Returns all `.proto` file paths for a populated package
    ///
    /// Gets the paths to all protobuf files in `proto/vendor/<package_name>/`.
    /// This is typically used after calling `populate()` to get the list of files
    /// that were synced.
    ///
    /// # Arguments
    ///
    /// * `manifest` - The package manifest to look up
    ///
    /// # Returns
    ///
    /// A sorted vector of paths to all `.proto` files in the package's vendor directory.
    pub async fn populated_files(&self, manifest: &PackageManifest) -> Vec<PathBuf> {
        self.collect(&self.populated_path(manifest), true).await
    }
}

pub struct Entry {
    /// Actual bytes of the file
    pub contents: Bytes,
    /// File metadata, like mtime, ...
    pub metadata: Option<std::fs::Metadata>,
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
