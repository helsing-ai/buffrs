// (c) Copyright 2023 Helsing GmbH. All rights reserved.

use std::{
    collections::HashMap,
    fmt::{self, Formatter},
    fs::File,
    io::{self, Cursor, Read, Write},
    ops::Deref,
    path::{Path, PathBuf},
    str::FromStr,
};

use async_recursion::async_recursion;
use bytes::{Buf, Bytes};
use eyre::{ensure, Context, ContextCompat};
use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};
use tokio::fs;
use walkdir::WalkDir;

use crate::{
    lock::{LockedPackage, Lockfile},
    manifest::{self, Dependency, Manifest, MANIFEST_FILE},
    registry::Registry,
};

/// IO abstraction layer over local `buffrs` package store
pub struct PackageStore;

impl PackageStore {
    /// Path to the proto directory
    pub const PROTO_PATH: &str = "proto";
    /// Path to the dependency store
    pub const PROTO_VENDOR_PATH: &str = "proto/vendor";

    /// Creates the expected directory structure for `buffrs`
    pub async fn create() -> eyre::Result<()> {
        let create = |dir: &'static str| async move {
            fs::create_dir_all(dir).await.wrap_err(eyre::eyre!(
                "Failed to create dependency folder {}",
                Path::new(dir).canonicalize()?.to_string_lossy()
            ))
        };

        create(Self::PROTO_PATH).await?;
        create(Self::PROTO_VENDOR_PATH).await?;

        Ok(())
    }

    /// Clears all packages from the file system
    pub async fn clear() -> eyre::Result<()> {
        fs::remove_dir_all(Self::PROTO_VENDOR_PATH)
            .await
            .wrap_err("Failed to uninstall dependencies")
    }

    /// Unpacks a package into a local directory
    pub async fn unpack(package: &Package) -> eyre::Result<()> {
        let mut tar = Vec::new();

        let mut gz = flate2::read::GzDecoder::new(package.tgz.clone().reader());

        gz.read_to_end(&mut tar)
            .wrap_err("Failed to decompress package")?;

        let mut tar = tar::Archive::new(Bytes::from(tar).reader());

        let pkg_dir =
            Path::new(Self::PROTO_VENDOR_PATH).join(package.manifest.package.name.as_str());

        fs::remove_dir_all(&pkg_dir).await.ok();

        fs::create_dir_all(&pkg_dir)
            .await
            .wrap_err("Failed to install dependencies")?;

        tar.unpack(pkg_dir.clone()).wrap_err(format!(
            "Failed to unpack tar of {}",
            package.manifest.package.name
        ))?;

        tracing::debug!(
            ":: unpacked {}@{} into {}",
            package.manifest.package.name,
            package.manifest.package.version,
            pkg_dir.display()
        );

        Ok(())
    }

    /// Uninstalls a package from the local file system
    pub async fn uninstall(package: &PackageName) -> eyre::Result<()> {
        let pkg_dir = Path::new(Self::PROTO_VENDOR_PATH).join(package.as_str());

        fs::remove_dir_all(&pkg_dir)
            .await
            .wrap_err_with(|| format!("Failed to uninstall {package}"))
    }

    /// Resolves a package in the local file system
    pub async fn resolve(package: &PackageName) -> eyre::Result<Manifest> {
        let manifest = Self::locate(package).join(MANIFEST_FILE);

        let manifest: String = fs::read_to_string(&manifest).await.wrap_err(format!(
            "Failed to locate local manifest for package: {package}"
        ))?;

        Manifest::try_from(manifest)
            .wrap_err(format!("Malformed manifest of package {package}"))
            .map(Manifest::from)
    }

    /// Packages a release from the local file system state
    pub async fn release() -> eyre::Result<Package> {
        let manifest = Manifest::read().await?;

        let pkg_manifest = manifest.package.clone();

        if let PackageType::Lib = pkg_manifest.r#type {
            ensure!(
                manifest.dependencies.is_empty(),
                "Libraries can not have any dependencies"
            );
        }

        for dependency in manifest.dependencies.iter() {
            let resolved = Self::resolve(&dependency.package)
                .await
                .wrap_err("Failed to resolve dependency locally")?;

            let package = resolved.package;

            ensure!(
                package.r#type == PackageType::Lib,
                "Depending on api packages is prohibited"
            );
        }

        let pkg_path = fs::canonicalize(&Self::PROTO_PATH)
            .await
            .wrap_err_with(|| {
                format!(
                    "Failed to locate package folder (expected directory {} to be present)",
                    Self::PROTO_PATH
                )
            })?;

        let mut archive = tar::Builder::new(Vec::new());

        let manifest_bytes = {
            let as_str: String = manifest
                .clone()
                .try_into()
                .wrap_err("failed to render manifest as TOML")?;
            as_str.into_bytes()
        };

        let mut header = tar::Header::new_gnu();
        header.set_size(
            manifest_bytes
                .len()
                .try_into()
                .wrap_err("Failed to pack tar")?,
        );
        header.set_mode(0o444);
        archive
            .append_data(&mut header, MANIFEST_FILE, Cursor::new(manifest_bytes))
            .wrap_err("Failed to add manifest to release")?;

        for entry in Self::collect(&pkg_path).await {
            let file = File::open(&entry)
                .wrap_err_with(|| format!("Failed to open entry {}", entry.display()))?;

            let mut header = tar::Header::new_gnu();
            header.set_mode(0o444);
            header.set_size(
                file.metadata()
                    .wrap_err_with(|| {
                        format!("Failed to fetch metadata for entry {}", entry.display())
                    })?
                    .len(),
            );

            archive
                .append_data(
                    &mut header,
                    entry.strip_prefix(&pkg_path).wrap_err_with(|| {
                        format!("Failed to resolve path for entry {}", entry.display())
                    })?,
                    file,
                )
                .wrap_err_with(|| {
                    format!("Failed to add proto {} to release tar", entry.display())
                })?;
        }

        archive.finish()?;

        let tar = archive.into_inner().wrap_err("Failed to pack tar")?;

        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());

        encoder
            .write_all(&tar)
            .wrap_err("Failed to compress release")?;

        let tgz = encoder
            .finish()
            .wrap_err("Failed to release package")?
            .into();

        tracing::info!(":: packaged {}@{}", pkg_manifest.name, pkg_manifest.version);

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
    pub fn lock(&self, repository: String) -> LockedPackage {
        LockedPackage::lock(self, repository)
    }
}

impl TryFrom<Bytes> for Package {
    type Error = eyre::Error;

    fn try_from(tgz: Bytes) -> eyre::Result<Self> {
        let mut tar = Vec::new();

        let mut gz = flate2::read::GzDecoder::new(tgz.clone().reader());

        gz.read_to_end(&mut tar)
            .wrap_err("Failed to decompress package")?;

        let mut tar = tar::Archive::new(Bytes::from(tar).reader());

        let manifest = tar
            .entries()?
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
            .wrap_err("Failed to find manifest in package")?;

        let manifest = manifest.bytes().collect::<io::Result<Vec<_>>>()?;
        let manifest = Manifest::try_from(String::from_utf8(manifest)?)
            .wrap_err("Failed to parse manifest")?;

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

impl TryFrom<String> for PackageName {
    type Error = eyre::Error;

    fn try_from(value: String) -> eyre::Result<Self> {
        ensure!(
            value.len() > 2,
            "Package names must be at least three chars long"
        );

        ensure!(
            value
                .chars()
                .all(|c| (c.is_ascii_alphanumeric() && c.is_ascii_lowercase()) || c == '-'),
            "Invalid package name: {value} - only ASCII lowercase alphanumeric characters and dashes are accepted",
        );
        ensure!(
            value
                .get(0..1)
                .wrap_err("Expected package name to be non empty")?
                .chars()
                .all(|c| c.is_ascii_alphabetic()),
            "Package names must begin with an alphabetic letter"
        );

        Ok(Self(value))
    }
}

impl TryFrom<&str> for PackageName {
    type Error = eyre::Error;

    fn try_from(value: &str) -> eyre::Result<Self> {
        Self::try_from(value.to_string())
    }
}

impl TryFrom<&String> for PackageName {
    type Error = eyre::Error;

    fn try_from(value: &String) -> eyre::Result<Self> {
        Self::try_from(value.to_owned())
    }
}

impl FromStr for PackageName {
    type Err = eyre::Error;

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
    /// The repository the package was downloaded from
    pub repository: String,
    /// Packages that requested this dependency (and what versions they accept)
    pub dependants: Vec<Dependant>,
    /// Transitive dependencies
    pub depends_on: Vec<PackageName>,
}

/// Represents a requestor of the associated dependency
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

impl DependencyGraph {
    /// Recursively resolves dependencies from the manifest to build a dependency graph
    pub async fn from_manifest<R: Registry + Sync>(
        manifest: &Manifest,
        lockfile: &Lockfile,
        registry: &R,
    ) -> eyre::Result<Self> {
        let name = manifest.package.name.clone();

        let mut entries = HashMap::new();

        for dependency in &manifest.dependencies {
            Self::process_dependency(
                name.clone(),
                dependency.clone(),
                lockfile,
                registry,
                &mut entries,
            )
            .await?;
        }

        Ok(Self { entries })
    }

    #[async_recursion]
    async fn process_dependency<R: Registry + Sync>(
        name: PackageName,
        dependency: Dependency,
        lockfile: &Lockfile,
        registry: &R,
        entries: &mut HashMap<PackageName, ResolvedDependency>,
    ) -> eyre::Result<()> {
        let version_req = dependency.manifest.version.clone();

        if let Some(entry) = entries.get_mut(&dependency.package) {
            ensure!(version_req.matches(entry.package.version()), "A dependency of your project requires {}@{} which collides with {}@{} required by {}", dependency.package, dependency.manifest.version, entry.package.name(), entry.package.version(), entry.dependants[0].name);

            entry.dependants.push(Dependant { name, version_req });
        } else {
            let dependency_repo = dependency.manifest.repository.clone();
            let dependency_pkg = Self::resolve(dependency, registry, lockfile).await?;
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
                    repository: dependency_repo,
                    dependants: vec![Dependant { name, version_req }],
                    depends_on: sub_dependency_names,
                },
            );

            for sub_dependency in sub_dependencies {
                Self::process_dependency(
                    dependency_name.clone(),
                    sub_dependency,
                    lockfile,
                    registry,
                    entries,
                )
                .await?;
            }
        }

        Ok(())
    }

    async fn resolve<R: Registry>(
        dependency: Dependency,
        registry: &R,
        lockfile: &Lockfile,
    ) -> eyre::Result<Package> {
        if let Some(local_locked) = lockfile.get(&dependency.package) {
            ensure!(
                dependency.manifest.version.matches(&local_locked.version),
                "Dependency {} cannot be satisfied - requested {}, but version {} is locked.",
                &dependency.package,
                &dependency.manifest.version,
                &local_locked.version
            );

            let remote_package = registry.download(local_locked.as_dependency()).await?;
            let remote_locked = remote_package.lock(dependency.manifest.repository);
            local_locked.validate(&remote_locked).wrap_err_with(|| {
                format!(
                    "Lockfile validation failed for dependency {}@{}",
                    local_locked.name, local_locked.version
                )
            })?;

            Ok(remote_package)
        } else {
            registry.download(dependency).await
        }
    }

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
