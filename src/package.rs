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
    collections::HashMap,
    fmt::{self, Formatter},
    fs::File,
    io::{self, Cursor, Read, Write},
    ops::Deref,
    path::{Path, PathBuf},
    str::FromStr,
    sync::Arc,
};

use async_recursion::async_recursion;
use bytes::{Buf, Bytes};
use displaydoc::Display;
use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::fs;
use walkdir::WalkDir;

use crate::{
    credentials::Credentials,
    errors::{DeserializationError, SerializationError},
    lock::{self, LockedPackage, Lockfile},
    manifest::{self, Dependency, Manifest, MANIFEST_FILE},
    registry::{Artifactory, Registry, RegistryUri},
};

/// IO abstraction layer over local `buffrs` package store
pub struct PackageStore;

/// failed to created {dir} directory
#[derive(Error, Display, Debug)]
pub struct CreateStoreError {
    dir: &'static str,
    source: std::io::Error,
}

impl PackageStore {
    /// Path to the proto directory
    pub const PROTO_PATH: &str = "proto";
    /// Path to the dependency store
    pub const PROTO_VENDOR_PATH: &str = "proto/vendor";

    /// Creates the expected directory structure for `buffrs`
    pub async fn create() -> Result<(), CreateStoreError> {
        let create = |dir: &'static str| async move {
            fs::create_dir_all(dir)
                .await
                .map_err(|source| CreateStoreError { dir, source })
        };

        create(Self::PROTO_PATH).await?;
        create(Self::PROTO_VENDOR_PATH).await?;

        Ok(())
    }

    /// Clears all packages from the file system
    pub async fn clear() -> Result<(), std::io::Error> {
        fs::remove_dir_all(Self::PROTO_VENDOR_PATH).await
    }
}

#[derive(Error, Display, Debug)]
#[allow(missing_docs)]
pub enum UnpackError {
    /// failed to decompress package
    Decompress(std::io::Error),
    /// failed to create the target dependency directory
    CreateDir(std::io::Error),
    /// failed to extract the tarball contents
    Extract(std::io::Error),
}

impl PackageStore {
    /// Unpacks a package into a local directory
    pub async fn unpack(package: &Package) -> Result<(), UnpackError> {
        let mut tar = Vec::new();

        let mut gz = flate2::read::GzDecoder::new(package.tgz.clone().reader());

        gz.read_to_end(&mut tar).map_err(UnpackError::Decompress)?;

        let mut tar = tar::Archive::new(Bytes::from(tar).reader());

        let pkg_dir =
            Path::new(Self::PROTO_VENDOR_PATH).join(package.manifest.package.name.as_str());

        fs::remove_dir_all(&pkg_dir).await.ok();

        fs::create_dir_all(&pkg_dir)
            .await
            .map_err(UnpackError::CreateDir)?;

        tar.unpack(pkg_dir.clone()).map_err(UnpackError::Extract)?;

        tracing::debug!(
            ":: unpacked {}@{} into {}",
            package.manifest.package.name,
            package.manifest.package.version,
            pkg_dir.display()
        );

        Ok(())
    }
}

/// failed to uninstall {package}
#[derive(Error, Display, Debug)]
pub struct UninstallError {
    package: PackageName,
    source: std::io::Error,
}

impl PackageStore {
    /// Uninstalls a package from the local file system
    pub async fn uninstall(package: &PackageName) -> Result<(), UninstallError> {
        let pkg_dir = Path::new(Self::PROTO_VENDOR_PATH).join(package.as_str());

        fs::remove_dir_all(&pkg_dir)
            .await
            .map_err(|source| UninstallError {
                package: package.clone(),
                source,
            })
    }

    /// Resolves a package in the local file system
    pub async fn resolve(package: &PackageName) -> Result<Manifest, manifest::ReadError> {
        Manifest::read_from(Self::locate(package).join(MANIFEST_FILE)).await
    }
}

#[derive(Error, Display, Debug)]
#[allow(missing_docs)]
pub enum ReleaseError {
    /// failed to load manifest
    ManifestReadError(#[from] manifest::ReadError),
    /// packages with type `impl` cannot be published
    InvalidPackageType,
    /// library packages cannot have any dependencies
    LibWithDependencies,
    /// depending on API packages is not allowed for {0} packages
    ApiDependency(PackageType),
    /// io error: {message}
    Io {
        message: String,
        source: std::io::Error,
    },
    /// failed to serialized the package manifest
    Serialize(#[from] SerializationError),
    /// serialized manifest was too large to fit in a tarball
    ManifestTooLarge,
}

impl PackageStore {
    /// Packages a release from the local file system state
    pub async fn release() -> Result<Package, ReleaseError> {
        let manifest = Manifest::read().await?;

        if manifest.package.kind == PackageType::Impl {
            return Err(ReleaseError::InvalidPackageType);
        }

        if matches!(manifest.package.kind, PackageType::Lib) && !manifest.dependencies.is_empty() {
            return Err(ReleaseError::LibWithDependencies);
        }

        for dependency in manifest.dependencies.iter() {
            let resolved = Self::resolve(&dependency.package).await?;

            if resolved.package.kind != PackageType::Lib {
                return Err(ReleaseError::ApiDependency(resolved.package.kind));
            }
        }

        let pkg_path = fs::canonicalize(&Self::PROTO_PATH)
            .await
            .map_err(|source| ReleaseError::Io {
                message: format!(
                    "failed to locate package folder (expected directory {} to be present)",
                    Self::PROTO_PATH
                ),
                source,
            })?;

        let mut archive = tar::Builder::new(Vec::new());

        let manifest_bytes = {
            let as_str: String = manifest
                .clone()
                .try_into()
                .map_err(SerializationError::from)?;
            as_str.into_bytes()
        };

        let mut header = tar::Header::new_gnu();
        header.set_size(
            manifest_bytes
                .len()
                .try_into()
                .map_err(|_| ReleaseError::ManifestTooLarge)?,
        );
        header.set_mode(0o444);
        archive
            .append_data(&mut header, MANIFEST_FILE, Cursor::new(manifest_bytes))
            .map_err(|source| ReleaseError::Io {
                message: "failed to add manifest to release".into(),
                source,
            })?;

        for entry in Self::collect(&pkg_path).await {
            let file = File::open(&entry).map_err(|source| ReleaseError::Io {
                message: format!("failed to open file {}", entry.display()),
                source,
            })?;

            let mut header = tar::Header::new_gnu();
            header.set_mode(0o444);
            header.set_size(
                file.metadata()
                    .map_err(|source| ReleaseError::Io {
                        message: format!("failed to fetch metadata for entry {}", entry.display()),
                        source,
                    })?
                    .len(),
            );

            archive
                .append_data(
                    &mut header,
                    entry.strip_prefix(&pkg_path).expect(
                        "unexpected error: collected file path is not under package prefix",
                    ),
                    file,
                )
                .map_err(|source| ReleaseError::Io {
                    message: format!("failed to add proto {} to release tar", entry.display()),
                    source,
                })?;
        }

        let tar = archive.into_inner().map_err(|source| ReleaseError::Io {
            message: "failed to assemble tar package".into(),
            source,
        })?;

        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());

        encoder.write_all(&tar).map_err(|source| ReleaseError::Io {
            message: "failed to compress release".into(),
            source,
        })?;

        let tgz = encoder
            .finish()
            .map_err(|source| ReleaseError::Io {
                message: "failed to finalize package".into(),
                source,
            })?
            .into();

        tracing::info!(
            ":: packaged {}@{}",
            manifest.package.name,
            manifest.package.version
        );

        Ok(Package::new(manifest, tgz))
    }

    /// Directory for the vendored installation of a package
    pub fn locate(package: &PackageName) -> PathBuf {
        PathBuf::from(Self::PROTO_VENDOR_PATH).join(package.as_str())
    }

    /// Collect .proto files in a given path whilst excluding vendored ones
    pub async fn collect(path: &Path) -> Vec<PathBuf> {
        let vendor_path = fs::canonicalize(&Self::PROTO_VENDOR_PATH)
            .await
            .unwrap_or(Self::PROTO_VENDOR_PATH.into());

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
    pub fn lock(
        &self,
        registry: RegistryUri,
        repository: String,
        dependants: usize,
    ) -> LockedPackage {
        LockedPackage::lock(self, registry, repository, dependants)
    }
}

#[derive(Error, Display, Debug)]
#[allow(missing_docs)]
pub enum DecodePackageError {
    /// io error: {message}
    Io {
        message: String,
        source: std::io::Error,
    },
    /// package is missing a manifest file
    MissingManifest,
    /// manifest has invalid character encoding (not UTF-8)
    InvalidEncoding,
    /// failed to parse the package manifest
    Deserialize(#[from] DeserializationError),
}

impl TryFrom<Bytes> for Package {
    type Error = DecodePackageError;

    fn try_from(tgz: Bytes) -> Result<Self, Self::Error> {
        let mut tar = Vec::new();

        let mut gz = flate2::read::GzDecoder::new(tgz.clone().reader());

        gz.read_to_end(&mut tar)
            .map_err(|source| DecodePackageError::Io {
                message: "failed to decompress package".into(),
                source,
            })?;

        let mut tar = tar::Archive::new(Bytes::from(tar).reader());

        let manifest = tar
            .entries()
            .map_err(|source| DecodePackageError::Io {
                message: "corrupted tar package".into(),
                source,
            })?
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
            .ok_or(DecodePackageError::MissingManifest)?;

        let manifest = manifest
            .bytes()
            .collect::<io::Result<Vec<_>>>()
            .map_err(|source| DecodePackageError::Io {
                message: "failed to read manifest".into(),
                source,
            })?;
        let manifest = Manifest::try_from(
            String::from_utf8(manifest).map_err(|_| DecodePackageError::InvalidEncoding)?,
        )?;

        Ok(Self { manifest, tgz })
    }
}

/// Package types
#[derive(Copy, Clone, Debug, Hash, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum PackageType {
    /// A library package containing primitive type definitions
    Lib,
    /// An api package containing message and service definition
    Api,
    /// An implementation package that implements an api or library
    ///
    /// Note: Implementation packages can't be published via Buffrs
    Impl,
}

impl PackageType {
    /// Whether this package type is publishable
    pub fn publishable(&self) -> bool {
        *self != Self::Impl
    }

    /// Whether this package type is compilable
    pub fn compilable(&self) -> bool {
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
        write!(f, "{}", serde_typename::to_str(self).unwrap_or_default())
    }
}

impl Default for PackageType {
    fn default() -> Self {
        Self::Impl
    }
}

/// A `buffrs` package name for parsing and type safety
#[derive(Clone, Hash, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(try_from = "String", into = "String")]
pub struct PackageName(String);

#[derive(Error, Display, Debug)]
#[allow(missing_docs)]
pub enum InvalidPackageName {
    /// package names must be at least three chars long
    InvalidLength(usize),
    /// invalid package name: {0} - only ASCII lowercase alphanumeric characters and dashes are accepted
    InvalidCharacters(String),
    /// package names must begin with an alphabetic letter
    InvalidFirstCharacter,
}

impl TryFrom<String> for PackageName {
    type Error = InvalidPackageName;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.len() < 3 {
            return Err(InvalidPackageName::InvalidLength(value.len()));
        }

        if !value
            .chars()
            .all(|c| (c.is_ascii_alphanumeric() && !c.is_ascii_uppercase()) || c == '-')
        {
            return Err(InvalidPackageName::InvalidCharacters(value));
        }

        const UNEXPECTED_MSG: &str =
            "Unexpected error: package name length should be validated prior to first character check";

        if !value.chars().next().expect(UNEXPECTED_MSG).is_alphabetic() {
            return Err(InvalidPackageName::InvalidFirstCharacter);
        }

        Ok(Self(value))
    }
}

impl TryFrom<&str> for PackageName {
    type Error = InvalidPackageName;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::try_from(value.to_string())
    }
}

impl TryFrom<&String> for PackageName {
    type Error = InvalidPackageName;

    fn try_from(value: &String) -> Result<Self, Self::Error> {
        Self::try_from(value.to_owned())
    }
}

impl FromStr for PackageName {
    type Err = InvalidPackageName;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::try_from(s)
    }
}

impl From<PackageName> for String {
    fn from(s: PackageName) -> Self {
        s.to_string()
    }
}

impl Deref for PackageName {
    type Target = String;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl fmt::Display for PackageName {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl fmt::Debug for PackageName {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_tuple("PackageName")
            .field(&format!("{self}"))
            .finish()
    }
}

/// Represents a dependency contextualized by the current dependency graph
pub struct ResolvedDependency {
    /// The materialized package as downloaded from the registry
    pub package: Package,
    /// The registry the package was downloaded from
    pub registry: RegistryUri,
    /// The repository in the registry where the package can be found
    pub repository: String,
    /// Packages that requested this dependency (and what versions they accept)
    pub dependants: Vec<Dependant>,
    /// Transitive dependencies
    pub depends_on: Vec<PackageName>,
}

/// Represents a requester of the associated dependency
pub struct Dependant {
    /// Package that requested the dependency
    pub name: PackageName,
    /// Version requirement
    pub version_req: VersionReq,
}

/// Represents direct and transitive dependencies of the root package
pub struct DependencyGraph {
    entries: HashMap<PackageName, ResolvedDependency>,
}

#[derive(Error, Display, Debug)]
#[allow(missing_docs)]
pub enum PackageResolutionError {
    /// dependency {name} cannot be satisfied - requested {requested}, but version {locked} is locked
    ConstraintViolation {
        name: PackageName,
        requested: VersionReq,
        locked: Version,
    },
    /// mismatched registry detected for dependency {name} - requested {requested} but lockfile requires {locked}
    MismatchedRegistry {
        name: PackageName,
        requested: RegistryUri,
        locked: RegistryUri,
    },
    /// failed to download dependency {name}@{version} from the registry
    DownloadError {
        name: PackageName,
        version: VersionReq,
        source: eyre::Report,
    },
    /// the downloaded package did not match the lockfile entry
    ValidationError(#[from] lock::ValidationError),
}

#[derive(Error, Display, Debug)]
#[allow(missing_docs)]
pub enum DependencyGraphBuildError {
    /// could not materialize a package from its locator
    PackageResolution(#[from] PackageResolutionError),
    /// a dependency of your project requires {package}@{curr} which collides with {package}@{other} required by {dependant}
    ConstraintViolation {
        package: PackageName,
        dependant: PackageName,
        curr: VersionReq,
        other: Version,
    },
}

impl DependencyGraph {
    /// Recursively resolves dependencies from the manifest to build a dependency graph
    pub async fn from_manifest(
        manifest: &Manifest,
        lockfile: &Lockfile,
        credentials: &Arc<Credentials>,
    ) -> Result<Self, DependencyGraphBuildError> {
        let name = manifest.package.name.clone();

        let mut entries = HashMap::new();

        for dependency in &manifest.dependencies {
            Self::process_dependency(
                name.clone(),
                dependency.clone(),
                true,
                lockfile,
                credentials,
                &mut entries,
            )
            .await?;
        }

        Ok(Self { entries })
    }

    #[async_recursion]
    async fn process_dependency(
        name: PackageName,
        dependency: Dependency,
        is_root: bool,
        lockfile: &Lockfile,
        credentials: &Arc<Credentials>,
        entries: &mut HashMap<PackageName, ResolvedDependency>,
    ) -> Result<(), DependencyGraphBuildError> {
        let version_req = dependency.manifest.version.clone();
        if let Some(entry) = entries.get_mut(&dependency.package) {
            if !version_req.matches(entry.package.version()) {
                return Err(DependencyGraphBuildError::ConstraintViolation {
                    package: dependency.package,
                    dependant: entry.dependants[0].name.clone(),
                    curr: dependency.manifest.version,
                    other: entry.package.manifest.package.version.clone(),
                });
            }

            entry.dependants.push(Dependant { name, version_req });
        } else {
            let dependency_pkg =
                Self::resolve(dependency.clone(), is_root, lockfile, credentials).await?;

            let dependency_name = dependency_pkg.name().clone();
            let sub_dependencies = dependency_pkg.manifest.dependencies.clone();
            let sub_dependency_names: Vec<_> = sub_dependencies
                .iter()
                .map(|sub_dependency| sub_dependency.package.clone())
                .collect();

            entries.insert(
                dependency_name.clone(),
                ResolvedDependency {
                    package: dependency_pkg,
                    registry: dependency.manifest.registry,
                    repository: dependency.manifest.repository,
                    dependants: vec![Dependant { name, version_req }],
                    depends_on: sub_dependency_names,
                },
            );

            for sub_dependency in sub_dependencies {
                Self::process_dependency(
                    dependency_name.clone(),
                    sub_dependency,
                    false,
                    lockfile,
                    credentials,
                    entries,
                )
                .await?;
            }
        }

        Ok(())
    }

    async fn resolve(
        dependency: Dependency,
        is_root: bool,
        lockfile: &Lockfile,
        credentials: &Arc<Credentials>,
    ) -> Result<Package, PackageResolutionError> {
        if let Some(local_locked) = lockfile.get(&dependency.package) {
            if !dependency.manifest.version.matches(&local_locked.version) {
                return Err(PackageResolutionError::ConstraintViolation {
                    name: dependency.package,
                    requested: dependency.manifest.version,
                    locked: local_locked.version.clone(),
                });
            }

            if !is_root && dependency.manifest.registry == local_locked.registry {
                return Err(PackageResolutionError::MismatchedRegistry {
                    name: dependency.package,
                    requested: dependency.manifest.registry,
                    locked: local_locked.registry.clone(),
                });
            }

            let registry = Artifactory::new(dependency.manifest.registry.clone(), credentials)
                .map_err(|err| PackageResolutionError::DownloadError {
                    name: dependency.package.clone(),
                    version: dependency.manifest.version.clone(),
                    source: err.into(),
                })?;

            let package = registry
                .download(dependency.with_version(&local_locked.version))
                .await
                .map_err(|err| PackageResolutionError::DownloadError {
                    name: dependency.package,
                    version: dependency.manifest.version,
                    source: err.into(),
                })?;

            local_locked.validate(&package)?;

            Ok(package)
        } else {
            let registry = Artifactory::new(dependency.manifest.registry.clone(), credentials)
                .map_err(|err| PackageResolutionError::DownloadError {
                    name: dependency.package.clone(),
                    version: dependency.manifest.version.clone(),
                    source: err.into(),
                })?;
            registry.download(dependency.clone()).await.map_err(|err| {
                PackageResolutionError::DownloadError {
                    name: dependency.package,
                    version: dependency.manifest.version,
                    source: err.into(),
                }
            })
        }
    }

    /// Locates and returns a reference to a resolved dependency package by its name
    pub fn get(&self, name: &PackageName) -> Option<&ResolvedDependency> {
        self.entries.get(name)
    }
}

impl IntoIterator for DependencyGraph {
    type Item = ResolvedDependency;
    type IntoIter = std::collections::hash_map::IntoValues<PackageName, ResolvedDependency>;
    fn into_iter(self) -> Self::IntoIter {
        self.entries.into_values()
    }
}
