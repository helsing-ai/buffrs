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

use crate::{
    manifest::{MANIFEST_FILE, Manifest, PackageManifest, PackagesManifest},
    package::{Package, PackageName},
};
use ignore::{Match, WalkBuilder, overrides::OverrideBuilder, types::TypesBuilder};

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
            "unpacked {}@{} into {}",
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
    /// - The `Proto.toml` file is missing or corrupted
    /// - The manifest cannot be deserialized
    pub async fn resolve(&self, package: &PackageName) -> miette::Result<PackagesManifest> {
        let manifest_path = self.locate(package).join(MANIFEST_FILE);

        let manifest = Manifest::require_package_manifest(&manifest_path)
            .await
            .wrap_err({
                miette!(
                    "the package store is corrupted: `{}` is not present",
                    manifest_path.display()
                )
            })?;

        Ok(manifest)
    }

    /// Validates the protobuf files in a package against buffrs rules
    ///
    /// Runs validation rules on all `.proto` files in the package to ensure they
    /// conform to buffrs conventions (naming, package structure, etc.).
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
        let source_files = self.populated_files(manifest).await?;

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
        manifest: &PackagesManifest,
        preserve_mtime: bool,
    ) -> miette::Result<Package> {
        let pkg_path = self.proto_path();
        let mut entries = BTreeMap::new();

        let include = manifest.package.as_ref().and_then(|p| p.include.as_deref());
        let exclude = manifest
            .package
            .as_ref()
            .map(|p| p.exclude.as_slice())
            .unwrap_or(&[]);
        for entry in self.collect(&pkg_path, false, include, exclude).await? {
            let path = entry.strip_prefix(&pkg_path).into_diagnostic()?;
            let contents = tokio::fs::read(&entry)
                .await
                .into_diagnostic()
                .wrap_err_with(|| format!("failed to read {}", entry.display()))?;

            entries.insert(
                path.into(),
                Entry {
                    contents: contents.into(),
                    metadata: tokio::fs::metadata(&entry).await.ok(),
                },
            );
        }

        let package = Package::create(manifest.clone(), entries, preserve_mtime)?;

        tracing::info!("packaged {}@{}", package.name(), package.version());

        Ok(package)
    }

    /// Returns the installation directory path for a package
    ///
    /// Returns the path where a package is (or will be) installed in the vendor directory.
    /// The path format is `proto/vendor/<package_name>/`.
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
    /// * `vendored` - If `false`, excludes files from `proto/vendor/` (for collecting source files only), if `true`, includes all `.proto` files regardless of location
    pub async fn collect(
        &self,
        path: &Path,
        vendored: bool,
        include: Option<&[String]>,
        exclude: &[String],
    ) -> miette::Result<Vec<PathBuf>> {
        debug_assert!(
            include.is_none() || exclude.is_empty(),
            "include and exclude are mutually exclusive"
        );

        let mut builder = WalkBuilder::new(path);
        builder.standard_filters(false);

        let vendor_path = self.proto_vendor_path();

        if let Some(include) = include {
            // Use the inclusions list to select files via overrides
            let mut overrides_builder = OverrideBuilder::new(path);
            for glob in include {
                overrides_builder
                    .add(glob)
                    .into_diagnostic()
                    .wrap_err_with(|| format!("invalid include entry: {glob}"))?;
            }
            builder.overrides(
                overrides_builder
                    .build()
                    .into_diagnostic()
                    .wrap_err("failed to build include filter")?,
            );
            builder.filter_entry(move |e| {
                if vendored {
                    true
                } else {
                    !e.path().starts_with(&vendor_path)
                }
            });
        } else if !exclude.is_empty() {
            // Start from all files (no type filter) and exclude matches
            let mut overrides_builder = OverrideBuilder::new(path);
            for glob in exclude {
                overrides_builder
                    .add(glob)
                    .into_diagnostic()
                    .wrap_err_with(|| format!("invalid exclude entry: {glob}"))?;
            }
            let overrides = overrides_builder
                .build()
                .into_diagnostic()
                .wrap_err("failed to build exclude filter")?;
            builder.filter_entry(move |e| {
                let Some(ftype) = e.file_type() else {
                    // file_type() returns None for stdin or broken symlinks;
                    // exclude the entry in either case
                    return false;
                };
                let path = e.path();
                match overrides.matched(path, ftype.is_dir()) {
                    Match::None | Match::Ignore(_) => {
                        if vendored {
                            true
                        } else {
                            !path.starts_with(&vendor_path)
                        }
                    }
                    Match::Whitelist(_) => false,
                }
            });
        } else {
            // Default: include only .proto files
            let proto_types = {
                let mut types = TypesBuilder::new();
                types.add("proto", "*.proto").expect("valid name");
                types.select("proto");
                types.build().expect("no conflicting definitions")
            };
            builder.types(proto_types).filter_entry(move |e| {
                if vendored {
                    true
                } else {
                    !e.path().starts_with(&vendor_path)
                }
            });
        }

        let mut paths: Vec<_> = builder
            .build()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().map(|ft| ft.is_file()).unwrap_or(false))
            .map(|entry| entry.into_path())
            .collect();

        // to ensure determinism
        paths.sort();

        Ok(paths)
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
            .wrap_err_with(|| {
                format!(
                    "failed to check whether directory {} still exists",
                    target_dir.display()
                )
            })?
        {
            tokio::fs::remove_dir_all(&target_dir)
                .await
                .into_diagnostic()
                .wrap_err_with(|| {
                    format!(
                        "failed to remove directory {} and its contents",
                        target_dir.display()
                    )
                })?;
        }

        let include = manifest.include.as_deref();
        for entry in self
            .collect(&source_path, false, include, &manifest.exclude)
            .await?
        {
            let file_name = entry.strip_prefix(&source_path).into_diagnostic()?;
            let target_path = target_dir.join(file_name);

            let parent = target_path.parent().ok_or_else(|| {
                miette!("unexpected root path in target: {}", target_path.display())
            })?;
            tokio::fs::create_dir_all(parent)
                .await
                .into_diagnostic()
                .wrap_err_with(|| {
                    format!(
                        "failed to create directory {} and its parents",
                        parent.display()
                    )
                })?;

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
    /// # Returns
    ///
    /// A sorted vector of paths to all `.proto` files in the package's vendor directory.
    pub async fn populated_files(
        &self,
        manifest: &PackageManifest,
    ) -> miette::Result<Vec<PathBuf>> {
        // Don't re-apply include/exclude here: files were already filtered
        // by populate() when they were copied into the vendor directory.
        self.collect(&self.populated_path(manifest), true, None, &[])
            .await
    }
}

pub struct Entry {
    /// Actual bytes of the file
    pub contents: Bytes,
    /// File metadata, like mtime, ...
    pub metadata: Option<std::fs::Metadata>,
}

#[cfg(test)]
mod tests {
    use super::*;

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

    /// Helper to create a test directory structure for collect() tests:
    ///
    /// ```text
    /// proto/
    ///   hello.proto
    ///   subdir/
    ///     nested.proto
    ///   excluded.proto
    ///   vendor/
    ///     dep/
    ///       dep.proto
    /// ```
    fn setup_test_dir(tmp: &Path) -> PackageStore {
        let proto = tmp.join("proto");
        let subdir = proto.join("subdir");
        let vendor = proto.join("vendor");
        let dep = vendor.join("dep");

        std::fs::create_dir_all(&subdir).unwrap();
        std::fs::create_dir_all(&dep).unwrap();

        std::fs::write(proto.join("hello.proto"), "syntax = \"proto3\";").unwrap();
        std::fs::write(subdir.join("nested.proto"), "syntax = \"proto3\";").unwrap();
        std::fs::write(proto.join("excluded.proto"), "syntax = \"proto3\";").unwrap();
        std::fs::write(dep.join("dep.proto"), "syntax = \"proto3\";").unwrap();

        PackageStore::new(tmp.to_path_buf())
    }

    /// Strip the prefix and collect file names for easier assertion
    fn relative_paths(paths: &[PathBuf], base: &Path) -> Vec<String> {
        let mut result: Vec<String> = paths
            .iter()
            .map(|p| p.strip_prefix(base).unwrap().to_string_lossy().to_string())
            .collect();
        result.sort();
        result
    }

    #[tokio::test]
    async fn collect_default_only_proto_files() {
        let tmp = tempfile::tempdir().unwrap();
        let store = setup_test_dir(tmp.path());

        // Also create a non-proto file to verify it's excluded
        std::fs::write(store.proto_path().join("readme.txt"), "not a proto").unwrap();

        let paths = store
            .collect(&store.proto_path(), false, None, &[])
            .await
            .unwrap();

        let names = relative_paths(&paths, &store.proto_path());
        assert_eq!(
            names,
            vec!["excluded.proto", "hello.proto", "subdir/nested.proto"]
        );
    }

    #[tokio::test]
    async fn collect_with_include_pattern() {
        let tmp = tempfile::tempdir().unwrap();
        let store = setup_test_dir(tmp.path());

        let include = vec!["subdir/*.proto".to_string()];
        let paths = store
            .collect(&store.proto_path(), false, Some(&include), &[])
            .await
            .unwrap();

        let names = relative_paths(&paths, &store.proto_path());
        assert_eq!(names, vec!["subdir/nested.proto"]);
    }

    #[tokio::test]
    async fn collect_with_exclude_pattern() {
        let tmp = tempfile::tempdir().unwrap();
        let store = setup_test_dir(tmp.path());

        let exclude = vec!["excluded.proto".to_string()];
        let paths = store
            .collect(&store.proto_path(), false, None, &exclude)
            .await
            .unwrap();

        let names = relative_paths(&paths, &store.proto_path());
        assert_eq!(names, vec!["hello.proto", "subdir/nested.proto"]);
    }

    #[tokio::test]
    async fn collect_with_exclude_starts_from_all_files() {
        let tmp = tempfile::tempdir().unwrap();
        let store = setup_test_dir(tmp.path());

        // A non-.proto file should be picked up when exclude is set,
        // since exclude starts from the set of all files.
        std::fs::write(store.proto_path().join("readme.txt"), "hello").unwrap();

        let exclude = vec!["excluded.proto".to_string()];
        let paths = store
            .collect(&store.proto_path(), false, None, &exclude)
            .await
            .unwrap();

        let names = relative_paths(&paths, &store.proto_path());
        assert_eq!(
            names,
            vec!["hello.proto", "readme.txt", "subdir/nested.proto"]
        );
    }

    #[tokio::test]
    async fn collect_excludes_vendor_when_not_vendored() {
        let tmp = tempfile::tempdir().unwrap();
        let store = setup_test_dir(tmp.path());

        let paths = store
            .collect(&store.proto_path(), false, None, &[])
            .await
            .unwrap();

        let names = relative_paths(&paths, &store.proto_path());
        assert!(
            !names.iter().any(|n| n.starts_with("vendor/")),
            "vendor files should not be included when vendored=false"
        );
    }

    #[tokio::test]
    async fn collect_includes_vendor_when_vendored() {
        let tmp = tempfile::tempdir().unwrap();
        let store = setup_test_dir(tmp.path());

        let paths = store
            .collect(&store.proto_path(), true, None, &[])
            .await
            .unwrap();

        let names = relative_paths(&paths, &store.proto_path());
        assert!(
            names.iter().any(|n| n.starts_with("vendor/")),
            "vendor files should be included when vendored=true"
        );
    }

    #[tokio::test]
    async fn collect_returns_only_files() {
        let tmp = tempfile::tempdir().unwrap();
        let store = setup_test_dir(tmp.path());

        let paths = store
            .collect(&store.proto_path(), true, None, &[])
            .await
            .unwrap();

        for path in &paths {
            assert!(
                path.is_file(),
                "{} should be a file, not a directory",
                path.display()
            );
        }
    }

    #[tokio::test]
    async fn collect_returns_only_files_with_include() {
        let tmp = tempfile::tempdir().unwrap();
        let store = setup_test_dir(tmp.path());

        let include = vec!["**/*.proto".to_string()];
        let paths = store
            .collect(&store.proto_path(), true, Some(&include), &[])
            .await
            .unwrap();

        for path in &paths {
            assert!(
                path.is_file(),
                "{} should be a file, not a directory",
                path.display()
            );
        }
    }

    #[tokio::test]
    async fn collect_invalid_include_glob_returns_error() {
        let tmp = tempfile::tempdir().unwrap();
        let store = setup_test_dir(tmp.path());

        let include = vec!["[invalid".to_string()];
        let result = store
            .collect(&store.proto_path(), false, Some(&include), &[])
            .await;

        assert!(
            result.is_err(),
            "invalid include glob should produce an error"
        );
    }

    #[tokio::test]
    async fn collect_invalid_exclude_glob_returns_error() {
        let tmp = tempfile::tempdir().unwrap();
        let store = setup_test_dir(tmp.path());

        let exclude = vec!["[invalid".to_string()];
        let result = store
            .collect(&store.proto_path(), false, None, &exclude)
            .await;

        assert!(
            result.is_err(),
            "invalid exclude glob should produce an error"
        );
    }
}
