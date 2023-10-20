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
    fmt::{self, Formatter},
    io::{self, Cursor, Read, Write},
    ops::Deref,
    path::{Path, PathBuf},
    str::FromStr,
};

use bytes::{Buf, Bytes};
use miette::{ensure, miette, Context, IntoDiagnostic};
use semver::Version;
use serde::{Deserialize, Serialize};
use tokio::fs;
use walkdir::WalkDir;

use crate::{
    errors::{DeserializationError, SerializationError},
    lock::LockedPackage,
    manifest::{self, Manifest, MANIFEST_FILE},
    registry::RegistryUri,
    ManagedFile,
};

/// IO abstraction layer over local `buffrs` package store
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageStore {
    root: PathBuf,
}

impl PackageStore {
    /// Path to the proto directory
    const PROTO_PATH: &str = "proto";
    /// Path to the dependency store
    const PROTO_VENDOR_PATH: &str = "proto/vendor";

    fn new(root: PathBuf) -> Self {
        Self { root }
    }

    /// Open current directory.
    pub fn current() -> Result<Self, io::Error> {
        Ok(Self::new(current_dir()?))
    }

    /// Open given directory.
    pub fn open(path: &Path) -> Self {
        Self::new(path.into())
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
        let mut tar = Vec::new();

        let mut gz = flate2::read::GzDecoder::new(package.tgz.clone().reader());

        gz.read_to_end(&mut tar)
            .into_diagnostic()
            .wrap_err(miette!("failed to decompress package {}", package.name()))?;

        let mut tar = tar::Archive::new(Bytes::from(tar).reader());

        let pkg_dir = self.locate(&package.manifest.package.name);

        fs::remove_dir_all(&pkg_dir).await.ok();

        fs::create_dir_all(&pkg_dir)
            .await
            .into_diagnostic()
            .wrap_err({
                miette!(
                    "failed to create extraction directory for package {}",
                    package.name()
                )
            })?;

        tar.unpack(pkg_dir.clone()).into_diagnostic().wrap_err({
            miette!(
                "failed to extract package {} to {}",
                package.name(),
                pkg_dir.display()
            )
        })?;

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
        Manifest::read_from(self.locate(package).join(MANIFEST_FILE))
            .await
            .wrap_err(miette!("failed to resolve package {package}"))
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

        for entry in self.collect(&pkg_path).await {
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

    /// Collect .proto files in a given path whilst excluding vendored ones
    pub async fn collect(&self, path: &Path) -> Vec<PathBuf> {
        let vendor_path = self.proto_vendor_path();
        let mut paths: Vec<_> = WalkDir::new(path)
            .into_iter()
            .filter_map(Result::ok)
            .map(|entry| entry.into_path())
            .filter(|path| !path.starts_with(&vendor_path))
            .filter(|path| {
                let ext = path.extension().map(|s| s.to_str());

                matches!(ext, Some(Some("proto")))
            })
            .collect();

        paths.sort(); // to ensure determinism

        paths
    }
}

/// An in memory representation of a `buffrs` package
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Package {
    /// Manifest of the package
    pub manifest: Manifest,
    /// The `tar.gz` archive containing the protocol buffers
    pub tgz: Bytes,
}

impl Package {
    /// Creates a new package
    pub fn new(manifest: Manifest, tgz: Bytes) -> Self {
        Self { manifest, tgz }
    }

    /// Create new [`Package`] from [`Manifest`] and list of files.
    ///
    /// This intentionally uses a [`BTreeMap`] to ensure that the list of files is sorted
    /// lexicographically. This ensures a reproducible output.
    pub fn create(manifest: Manifest, files: BTreeMap<PathBuf, Bytes>) -> miette::Result<Self> {
        let mut archive = tar::Builder::new(Vec::new());

        let manifest_bytes = {
            let as_str: String = manifest
                .clone()
                .try_into()
                .into_diagnostic()
                .wrap_err(SerializationError(ManagedFile::Manifest))?;
            as_str.into_bytes()
        };

        let mut header = tar::Header::new_gnu();
        header.set_size(
            manifest_bytes
                .len()
                .try_into()
                .into_diagnostic()
                .wrap_err(miette!(
                    "serialized manifest was too large to fit in a tarball"
                ))?,
        );
        header.set_mode(0o444);
        archive
            .append_data(&mut header, MANIFEST_FILE, Cursor::new(manifest_bytes))
            .into_diagnostic()
            .wrap_err(miette!("failed to add manifest to release"))?;

        for (name, contents) in &files {
            let mut header = tar::Header::new_gnu();
            header.set_mode(0o444);
            header.set_size(contents.len() as u64);
            archive
                .append_data(&mut header, name, &contents[..])
                .into_diagnostic()
                .wrap_err(miette!("failed to add proto {name:?} to release tar"))?;
        }

        let tar = archive
            .into_inner()
            .into_diagnostic()
            .wrap_err(miette!("failed to assemble tar package"))?;

        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());

        encoder
            .write_all(&tar)
            .into_diagnostic()
            .wrap_err(miette!("failed to compress release"))?;

        let tgz = encoder
            .finish()
            .into_diagnostic()
            .wrap_err(miette!("failed to finalize package"))?
            .into();

        Ok(Package::new(manifest, tgz))
    }

    /// Load a package from a precompressed archive.
    fn parse(tgz: Bytes) -> miette::Result<Self> {
        let mut tar = Vec::new();

        let mut gz = flate2::read::GzDecoder::new(tgz.clone().reader());

        gz.read_to_end(&mut tar)
            .into_diagnostic()
            .wrap_err(miette!("failed to decompress package"))?;

        let mut tar = tar::Archive::new(Bytes::from(tar).reader());

        let manifest = tar
            .entries()
            .into_diagnostic()
            .wrap_err(miette!("corrupted tar package"))?
            .filter_map(|entry| entry.ok())
            .find(|entry| {
                entry
                    .path()
                    .ok()
                    // TODO(rfink): The following line is a bug since it checks whether
                    //  actual path (relative to the process pwd) is a file, *not* whether
                    //  the tar entry would be a file if unpacked
                    // .filter(|path| path.is_file())
                    .filter(|path| path.ends_with(manifest::MANIFEST_FILE))
                    .is_some()
            })
            .ok_or_else(|| miette!("missing manifest"))?;

        let manifest = manifest
            .bytes()
            .collect::<io::Result<Vec<_>>>()
            .into_diagnostic()
            .wrap_err(DeserializationError(ManagedFile::Manifest))?;
        let manifest = String::from_utf8(manifest)
            .into_diagnostic()
            .wrap_err(miette!("manifest has invalid character encoding"))?
            .parse()
            .into_diagnostic()?;

        Ok(Self { manifest, tgz })
    }

    /// The name of this package
    #[inline]
    pub fn name(&self) -> &PackageName {
        &self.manifest.package.name
    }

    /// The version of this package
    #[inline]
    pub fn version(&self) -> &Version {
        &self.manifest.package.version
    }

    /// Lock this package
    ///
    /// Note that despite returning a Result this function never fails
    pub fn lock(
        &self,
        registry: RegistryUri,
        repository: String,
        dependants: usize,
    ) -> miette::Result<LockedPackage> {
        LockedPackage::lock(self, registry, repository, dependants)
    }
}

impl TryFrom<Bytes> for Package {
    type Error = miette::Report;

    fn try_from(tgz: Bytes) -> Result<Self, Self::Error> {
        Package::parse(tgz)
    }
}

/// Package types
#[derive(
    Copy, Clone, Debug, Hash, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Default,
)]
#[serde(rename_all = "snake_case")]
pub enum PackageType {
    /// A library package containing primitive type definitions
    Lib,
    /// An api package containing message and service definition
    Api,
    /// An implementation package that implements an api or library
    ///
    /// Note: Implementation packages can't be published via Buffrs
    #[default]
    Impl,
}

impl PackageType {
    /// Whether this package type is publishable
    pub fn is_publishable(&self) -> bool {
        *self != Self::Impl
    }

    /// Whether this package type is compilable
    pub fn is_compilable(&self) -> bool {
        *self != Self::Impl
    }
}

impl FromStr for PackageType {
    type Err = serde_typename::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        serde_typename::from_str(s)
    }
}

impl fmt::Display for PackageType {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match serde_typename::to_str(self) {
            Ok(value) => f.write_str(value),
            Err(_error) => unreachable!(),
        }
    }
}

/// A `buffrs` package name for parsing and type safety
#[derive(Clone, Hash, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Debug)]
#[serde(try_from = "String", into = "String")]
pub struct PackageName(String);

/// Errors that can be generated parsing [`PackageName`][], see [`PackageName::new()`][].
#[derive(thiserror::Error, Debug, PartialEq)]
pub enum PackageNameError {
    /// Incorrect length.
    #[error("package name must be at least three chars long, but was {0:} long")]
    Length(usize),
    /// Invalid start character.
    #[error("package name must start with alphabetic character, but was {0:}")]
    InvalidStart(char),
    /// Invalid character.
    #[error("package name must consist of only ASCII lowercase and dashes, but contains {0:} at position {1:}")]
    InvalidCharacter(char, usize),
}

impl PackageName {
    /// New package name from string.
    pub fn new<S: Into<String>>(value: S) -> Result<Self, PackageNameError> {
        let value = value.into();
        Self::validate(&value)?;
        Ok(Self(value))
    }

    /// Determine if this character is allowed at the start of a package name.
    fn is_allowed_start(c: char) -> bool {
        c.is_alphabetic()
    }

    /// Determine if this character is allowed anywhere in a package name.
    fn is_allowed(c: char) -> bool {
        let is_ascii_lowercase_alphanumeric =
            |c: char| c.is_ascii_alphanumeric() && !c.is_ascii_uppercase();
        match c {
            '-' => true,
            c if is_ascii_lowercase_alphanumeric(c) => true,
            _ => false,
        }
    }

    /// Validate a package name.
    pub fn validate(name: &str) -> Result<(), PackageNameError> {
        // validate length
        if name.len() < 3 {
            return Err(PackageNameError::Length(name.len()));
        }

        // validate first character
        match name.chars().next() {
            Some(c) if Self::is_allowed_start(c) => {}
            Some(c) => return Err(PackageNameError::InvalidStart(c)),
            None => unreachable!(),
        }

        // validate all characters
        let illegal = name
            .chars()
            .enumerate()
            .find(|(_, c)| !Self::is_allowed(*c));
        if let Some((index, c)) = illegal {
            return Err(PackageNameError::InvalidCharacter(c, index));
        }

        Ok(())
    }
}

impl TryFrom<String> for PackageName {
    type Error = PackageNameError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl FromStr for PackageName {
    type Err = PackageNameError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        Self::new(input)
    }
}

impl From<PackageName> for String {
    fn from(s: PackageName) -> Self {
        s.to_string()
    }
}

impl Deref for PackageName {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl fmt::Display for PackageName {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn can_parse_package_name() {
        assert_eq!(PackageName::new("abc"), Ok(PackageName("abc".into())));
        assert_eq!(PackageName::new("a"), Err(PackageNameError::Length(1)));
        assert_eq!(
            PackageName::new("4abc"),
            Err(PackageNameError::InvalidStart('4'))
        );
        assert_eq!(
            PackageName::new("serde_typename"),
            Err(PackageNameError::InvalidCharacter('_', 5))
        );
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

    #[test]
    fn can_check_publishable() {
        assert!(PackageType::Lib.is_publishable());
        assert!(PackageType::Api.is_publishable());
        assert!(!PackageType::Impl.is_publishable());
    }

    #[test]
    fn can_check_compilable() {
        assert!(PackageType::Lib.is_compilable());
        assert!(PackageType::Api.is_compilable());
        assert!(!PackageType::Impl.is_compilable());
    }

    #[test]
    fn can_default_package_type() {
        assert_eq!(PackageType::default(), PackageType::Impl);
    }

    #[test]
    fn can_parse_package_type() {
        let types = [PackageType::Lib, PackageType::Api, PackageType::Impl];
        for typ in &types {
            let string = typ.to_string();
            let parsed: PackageType = string.parse().unwrap();
            assert_eq!(parsed, *typ);
        }
    }
}
