// (c) Copyright 2023 Helsing GmbH. All rights reserved.

use std::{
    fmt::{self, Formatter},
    io::{self, Cursor, Read, Write},
    ops::Deref,
    path::{Path, PathBuf},
    str::FromStr,
};

use bytes::{Buf, Bytes};
use eyre::{ensure, Context, ContextCompat};
use serde::{Deserialize, Serialize};
use tokio::fs;
use walkdir::WalkDir;

use crate::{
    manifest::{self, Dependency, Manifest, PackageManifest, RawManifest, MANIFEST_FILE},
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

        let pkg_dir = Path::new(Self::PROTO_VENDOR_PATH).join(package.manifest.name.as_str());

        fs::remove_dir_all(&pkg_dir).await.ok();

        fs::create_dir_all(&pkg_dir)
            .await
            .wrap_err("Failed to install dependencies")?;

        tar.unpack(pkg_dir.clone())
            .wrap_err(format!("Failed to unpack tar of {}", package.manifest.name))?;

        tracing::debug!(
            ":: unpacked {}@{} into {}",
            package.manifest.name,
            package.manifest.version,
            pkg_dir.display()
        );

        Ok(())
    }

    /// Installs a package and all of its dependency into the local filesystem
    pub async fn install<R: Registry>(dependency: Dependency, registry: R) -> eyre::Result<()> {
        let package = registry.download(dependency).await?;

        Self::unpack(&package).await?;

        let mut tree = format!(
            ":: installed {}@{}",
            package.manifest.name, package.manifest.version
        );

        let Manifest { dependencies, .. } = Self::resolve(&package.manifest.name).await?;

        let dependency_count = dependencies.len();

        for (index, dependency) in dependencies.into_iter().enumerate() {
            if let Ok(manifest) = Self::resolve(&dependency.package).await {
                let existing = manifest.package.wrap_err(eyre::eyre!(
                    "Found installed manifest for {} but it is malformed",
                    dependency.package,
                ))?;

                eyre::ensure!(
                    dependency.manifest.version.matches(&existing.version),
                    "A dependency of your project requires {}@{} which collides with {}@{} required by {}",
                    existing.name,
                    existing.version,
                    dependency.package,
                    dependency.manifest.version,
                    package.manifest.name,
                );
            }

            let dependency = registry.download(dependency).await?;

            Self::unpack(&dependency).await?;

            let tree_char = if index + 1 == dependency_count {
                '┗'
            } else {
                '┣'
            };

            tree.push_str(&format!(
                "\n   {tree_char} installed {}@{}",
                dependency.manifest.name, dependency.manifest.version
            ));
        }

        tracing::info!("{tree}");

        Ok(())
    }

    /// Uninstalls a package from the local file system
    pub async fn uninstall(package: &PackageId) -> eyre::Result<()> {
        let pkg_dir = Path::new(Self::PROTO_VENDOR_PATH).join(package.as_str());

        fs::remove_dir_all(&pkg_dir)
            .await
            .wrap_err("Failed to uninstall {dependency}")
    }

    /// Resolves a package in the local file system
    pub async fn resolve(package: &PackageId) -> eyre::Result<Manifest> {
        let manifest = Self::locate(package).join(MANIFEST_FILE);

        let manifest: String = fs::read_to_string(&manifest).await.wrap_err(format!(
            "Failed to locate local manifest for package: {package}"
        ))?;

        toml::from_str::<RawManifest>(&manifest)
            .wrap_err(format!("Malformed manifest of package {package}"))
            .map(Manifest::from)
    }

    /// Packages a release from the local file system state
    pub async fn release() -> eyre::Result<Package> {
        let manifest = Manifest::read().await?;

        let pkg = manifest
            .package
            .to_owned()
            .wrap_err("Releasing a package requires a package manifest")?;

        if let PackageType::Lib = pkg.r#type {
            ensure!(
                manifest.dependencies.is_empty(),
                "Libraries can not have any dependencies"
            );
        }

        for dependency in manifest.dependencies.iter() {
            let resolved = Self::resolve(&dependency.package)
                .await
                .wrap_err("Failed to resolve dependency locally")?;

            let package = resolved
                .package
                .wrap_err("Local dependencies must contain a package declaration")?;

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

        let manifest = toml::to_string_pretty(&RawManifest::from(manifest))
            .wrap_err("Failed to encode release manifest")?
            .as_bytes()
            .to_vec();

        let mut archive = tar::Builder::new(Vec::new());

        for entry in Self::collect(&pkg_path).await {
            archive
                .append_path_with_name(
                    &entry,
                    entry.file_name().wrap_err_with(|| {
                        format!("Failed to get filename of entry {}", entry.display())
                    })?,
                )
                .wrap_err_with(|| {
                    format!("Failed to add proto {} to release tar", entry.display())
                })?;
        }

        let mut header = tar::Header::new_gnu();

        header.set_size(manifest.len().try_into().wrap_err("Failed to pack tar")?);

        archive
            .append_data(&mut header, MANIFEST_FILE, Cursor::new(manifest))
            .wrap_err("Failed to add manifest to release")?;

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

        tracing::info!(":: packaged {}@{}", pkg.name, pkg.version);

        Ok(Package::new(pkg, tgz))
    }

    /// Directory for the vendored installation of a package
    pub fn locate(package: &PackageId) -> PathBuf {
        PathBuf::from(Self::PROTO_VENDOR_PATH).join(package.as_str())
    }

    /// Collect .proto files in a given path whilst excluding vendored ones
    pub async fn collect(path: &Path) -> Vec<PathBuf> {
        let vendor_path = fs::canonicalize(&Self::PROTO_VENDOR_PATH)
            .await
            .unwrap_or(Self::PROTO_VENDOR_PATH.into());

        WalkDir::new(path)
            .into_iter()
            .filter_map(Result::ok)
            .map(|entry| entry.into_path())
            .filter(|path| !path.starts_with(&vendor_path))
            .filter(|path| {
                let ext = path.extension().map(|s| s.to_str());

                matches!(ext, Some(Some("proto")))
            })
            .collect()
    }
}

/// An in memory representation of a `buffrs` package
#[derive(Clone, Debug, Hash, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct Package {
    /// Manifest of the package
    pub manifest: PackageManifest,
    /// The `tar.gz` archive containing the protocol buffers
    #[serde(skip)]
    pub tgz: Bytes,
}

impl Package {
    /// Creates a new package
    pub fn new(manifest: PackageManifest, tgz: Bytes) -> Self {
        Self { manifest, tgz }
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

        let manifest: Vec<u8> = manifest.bytes().collect::<io::Result<Vec<u8>>>()?;
        let manifest: RawManifest = toml::from_str(&String::from_utf8(manifest)?)
            .wrap_err("Failed to parse the manifest")?;

        let manifest = manifest
            .package
            .wrap_err("The package section is missing the manifest")?;

        Ok(Self { manifest, tgz })
    }
}

/// Package types
#[derive(Copy, Clone, Debug, Hash, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum PackageType {
    /// A library package containing primitive type definitions
    Lib,
    /// A api package containing message and service definition
    Api,
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

/// A `buffrs` package id for parsing and type safety
#[derive(Clone, Hash, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(try_from = "String", into = "String")]
pub struct PackageId(String);

impl TryFrom<String> for PackageId {
    type Error = eyre::Error;

    fn try_from(value: String) -> eyre::Result<Self> {
        ensure!(
            value.len() > 2,
            "Package ids must be at least three chars long"
        );

        ensure!(
            value
                .chars()
                .all(|c| (c.is_ascii_alphanumeric() && c.is_ascii_lowercase()) || c == '-'),
            "Package ids can only consist of lowercase alphanumeric ascii chars and dashes"
        );
        ensure!(
            value
                .get(0..1)
                .wrap_err("Expected package id to be non empty")?
                .chars()
                .all(|c| c.is_ascii_alphabetic()),
            "Package ids must begin with an alphabetic letter"
        );

        Ok(Self(value))
    }
}

impl TryFrom<&str> for PackageId {
    type Error = eyre::Error;

    fn try_from(value: &str) -> eyre::Result<Self> {
        Self::try_from(value.to_string())
    }
}

impl TryFrom<&String> for PackageId {
    type Error = eyre::Error;

    fn try_from(value: &String) -> eyre::Result<Self> {
        Self::try_from(value.to_owned())
    }
}

impl FromStr for PackageId {
    type Err = eyre::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::try_from(s)
    }
}

impl From<PackageId> for String {
    fn from(s: PackageId) -> Self {
        s.to_string()
    }
}

impl Deref for PackageId {
    type Target = String;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl fmt::Display for PackageId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl fmt::Debug for PackageId {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_tuple("PackageId")
            .field(&format!("{self}"))
            .finish()
    }
}
