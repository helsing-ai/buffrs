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
use miette::{ensure, miette, Context, Diagnostic, IntoDiagnostic};
use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::fs;
use walkdir::WalkDir;

use crate::{
    credentials::Credentials,
    errors::{DeserializationError, SerializationError},
    lock::{LockedPackage, Lockfile},
    managed_file::ManagedFile,
    manifest::{self, Dependency, Manifest, MANIFEST_FILE},
    registry::{Artifactory, RegistryUri},
};

/// IO abstraction layer over local `buffrs` package store
pub struct PackageStore;

impl PackageStore {
    /// Path to the proto directory
    pub const PROTO_PATH: &str = "proto";
    /// Path to the dependency store
    pub const PROTO_VENDOR_PATH: &str = "proto/vendor";

    /// Creates the expected directory structure for `buffrs`
    pub async fn create() -> miette::Result<()> {
        let create = |dir: &'static str| async move {
            fs::create_dir_all(dir)
                .await
                .into_diagnostic()
                .wrap_err_with(|| miette!("failed to create {dir} directory"))
        };

        create(Self::PROTO_PATH).await?;
        create(Self::PROTO_VENDOR_PATH).await?;

        Ok(())
    }
}

impl PackageStore {
    /// Clears all packages from the file system
    pub async fn clear() -> miette::Result<()> {
        match fs::remove_dir_all(Self::PROTO_VENDOR_PATH).await {
            Ok(()) => Ok(()),
            Err(err) if matches!(err.kind(), std::io::ErrorKind::NotFound) => {
                Err(miette!("directory {} not found", Self::PROTO_VENDOR_PATH))
            }
            Err(_) => Err(miette!(
                "failed to clear {} directory",
                Self::PROTO_VENDOR_PATH,
            )),
        }
    }
}

impl PackageStore {
    /// Unpacks a package into a local directory
    pub async fn unpack(package: &Package) -> miette::Result<()> {
        let mut tar = Vec::new();

        let mut gz = flate2::read::GzDecoder::new(package.tgz.clone().reader());

        gz.read_to_end(&mut tar)
            .into_diagnostic()
            .wrap_err_with(|| miette!("failed to decompress package {}", package.name()))?;

        let mut tar = tar::Archive::new(Bytes::from(tar).reader());

        let pkg_dir =
            Path::new(Self::PROTO_VENDOR_PATH).join(package.manifest.package.name.as_str());

        fs::remove_dir_all(&pkg_dir).await.ok();

        fs::create_dir_all(&pkg_dir)
            .await
            .into_diagnostic()
            .wrap_err_with(|| {
                miette!(
                    "failed to create extraction directory for package {}",
                    package.name()
                )
            })?;

        tar.unpack(pkg_dir.clone())
            .into_diagnostic()
            .wrap_err_with(|| {
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
}

impl PackageStore {
    /// Uninstalls a package from the local file system
    pub async fn uninstall(package: &PackageName) -> miette::Result<()> {
        let pkg_dir = Path::new(Self::PROTO_VENDOR_PATH).join(package.as_str());

        fs::remove_dir_all(&pkg_dir)
            .await
            .into_diagnostic()
            .wrap_err(miette!("failed to uninstall package {package}"))
    }

    /// Resolves a package in the local file system
    pub async fn resolve(package: &PackageName) -> miette::Result<Manifest> {
        Manifest::read_from(Self::locate(package).join(MANIFEST_FILE))
            .await
            .wrap_err(miette!("failed to resolve package {package}"))
    }
}

impl PackageStore {
    /// Packages a release from the local file system state
    pub async fn release() -> miette::Result<Package> {
        let manifest = Manifest::read().await?;

        ensure!(
            manifest.package.kind != PackageType::Impl,
            "packages with type `impl` cannot be published"
        );

        ensure!(
            !matches!(manifest.package.kind, PackageType::Lib) || manifest.dependencies.is_empty(),
            "library packages cannot have any dependencies"
        );

        for dependency in manifest.dependencies.iter() {
            let resolved = Self::resolve(&dependency.package).await?;

            ensure!(
                resolved.package.kind == PackageType::Lib,
                "depending on API packages is not allowed for {} packages",
                manifest.package.kind
            );
        }

        let pkg_path = fs::canonicalize(&Self::PROTO_PATH)
            .await
            .into_diagnostic()
            .wrap_err_with(|| {
                miette!(
                    "failed to locate package folder (expected directory {} to be present)",
                    Self::PROTO_PATH
                )
            })?;

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

        for entry in Self::collect(&pkg_path).await {
            let file = File::open(&entry)
                .into_diagnostic()
                .wrap_err_with(|| miette!("failed to open file {}", entry.display()))?;

            let mut header = tar::Header::new_gnu();
            header.set_mode(0o444);
            header.set_size(
                file.metadata()
                    .into_diagnostic()
                    .wrap_err_with(|| {
                        miette!("failed to fetch metadata for entry {}", entry.display())
                    })?
                    .len(),
            );

            archive
                .append_data(
                    &mut header,
                    entry
                        .strip_prefix(&pkg_path)
                        .into_diagnostic()
                        .wrap_err(miette!(
                            "unexpected error: collected file path is not under package prefix"
                        ))?,
                    file,
                )
                .into_diagnostic()
                .wrap_err_with(|| {
                    miette!("failed to add proto {} to release tar", entry.display())
                })?;
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
        let mut tar = Vec::new();

        let mut gz = flate2::read::GzDecoder::new(tgz.clone().reader());

        gz.read_to_end(&mut tar)
            .into_diagnostic()
            .wrap_err_with(|| miette!("failed to decompress package"))?;

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
            .wrap_err_with(|| DeserializationError(ManagedFile::Manifest))?;

        let manifest = String::from_utf8(manifest)
            .into_diagnostic()
            .wrap_err(miette!("manifest has invalid character encoding"))?
            .parse()
            .into_diagnostic()?;

        Ok(Self { manifest, tgz })
    }
}

impl From<&Package> for Bytes {
    fn from(value: &Package) -> Self {
        value.tgz.clone()
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

impl TryFrom<String> for PackageName {
    type Error = miette::Report;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        ensure!(
            value.len() >= 3,
            "package names must be at least three chars long"
        );

        let all_lower_alphanum = value
            .chars()
            .all(|c| (c.is_ascii_alphanumeric() && !c.is_ascii_uppercase()) || c == '-');

        ensure!(all_lower_alphanum, "invalid package name: {value} - only ASCII lowercase alphanumeric characters and dashes are accepted");

        const UNEXPECTED_MSG: &str =
            "unexpected error: package name length should be validated prior to first character check";

        ensure!(
            value
                .chars()
                .next()
                .ok_or(miette!(UNEXPECTED_MSG))?
                .is_alphabetic(),
            "package names must begin with an alphabetic letter"
        );

        Ok(Self(value))
    }
}

impl TryFrom<&str> for PackageName {
    type Error = miette::Report;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::try_from(value.to_string())
    }
}

impl TryFrom<&String> for PackageName {
    type Error = miette::Report;

    fn try_from(value: &String) -> Result<Self, Self::Error> {
        Self::try_from(value.to_owned())
    }
}

impl FromStr for PackageName {
    type Err = miette::Report;

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

#[derive(Error, Diagnostic, Debug)]
#[error("failed to download dependency {name}@{version} from the registry")]
struct DownloadError {
    name: PackageName,
    version: VersionReq,
}

impl DependencyGraph {
    /// Recursively resolves dependencies from the manifest to build a dependency graph
    pub async fn from_manifest(
        manifest: &Manifest,
        lockfile: &Lockfile,
        credentials: &Arc<Credentials>,
    ) -> miette::Result<Self> {
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
    ) -> miette::Result<()> {
        let version_req = dependency.manifest.version.clone();
        if let Some(entry) = entries.get_mut(&dependency.package) {
            ensure!(
                version_req.matches(entry.package.version()),
                "a dependency of your project requires {}@{} which collides with {}@{} required by {}", 
                    dependency.package,
                    dependency.manifest.version,
                    entry.dependants[0].name.clone(),
                    dependency.manifest.version,
                    entry.package.manifest.package.version.clone(),
            );

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
    ) -> miette::Result<Package> {
        if let Some(local_locked) = lockfile.get(&dependency.package) {
            ensure!(
                dependency.manifest.version.matches(&local_locked.version),
                "dependency {} cannot be satisfied - requested {}, but version {} is locked",
                dependency.package,
                dependency.manifest.version,
                local_locked.version,
            );

            ensure!(
                is_root || dependency.manifest.registry == local_locked.registry,
                "mismatched registry detected for dependency {} - requested {} but lockfile requires {}",
                    dependency.package,
                    dependency.manifest.registry,
                    local_locked.registry,
            );

            let registry = Artifactory::new(dependency.manifest.registry.clone(), credentials)
                .wrap_err_with(|| DownloadError {
                    name: dependency.package.clone(),
                    version: dependency.manifest.version.clone(),
                })?;

            let package = registry
                .download(dependency.with_version(&local_locked.version))
                .await
                .wrap_err_with(|| DownloadError {
                    name: dependency.package,
                    version: dependency.manifest.version,
                })?;

            local_locked.validate(&package)?;

            Ok(package)
        } else {
            let registry = Artifactory::new(dependency.manifest.registry.clone(), credentials)
                .wrap_err_with(|| DownloadError {
                    name: dependency.package.clone(),
                    version: dependency.manifest.version.clone(),
                })?;

            registry
                .download(dependency.clone())
                .await
                .wrap_err_with(|| DownloadError {
                    name: dependency.package,
                    version: dependency.manifest.version,
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
