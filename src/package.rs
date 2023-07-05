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

use crate::manifest::{self, Manifest, PackageManifest, RawManifest, MANIFEST_FILE};

/// IO abstraction layer over local `buffrs` package store
pub struct PackageStore;

impl PackageStore {
    /// Path to the proto directory
    pub const PROTO_PATH: &str = "proto";
    /// Path to the api directory
    pub const PROTO_API_PATH: &str = "proto/api";
    /// Path to the dependency store
    pub const PROTO_DEP_PATH: &str = "proto/dep";

    /// Creates the expected directory structure for `buffrs`
    pub async fn create(api: bool) -> eyre::Result<()> {
        if api {
            fs::create_dir_all(Self::PROTO_API_PATH)
                .await
                .wrap_err(eyre::eyre!(
                    "Failed to create api folder {}",
                    Path::new(Self::PROTO_API_PATH)
                        .canonicalize()?
                        .to_string_lossy()
                ))?;
        }

        fs::create_dir_all(Self::PROTO_DEP_PATH)
            .await
            .wrap_err(eyre::eyre!(
                "Failed to create dependency folder {}",
                Path::new(Self::PROTO_DEP_PATH)
                    .canonicalize()?
                    .to_string_lossy()
            ))
    }

    /// Clears all packages from the file system
    pub async fn clear() -> eyre::Result<()> {
        fs::remove_dir_all(Self::PROTO_DEP_PATH)
            .await
            .wrap_err("Failed to uninstall dependencies")
    }

    /// Installs a package into the local file system
    pub async fn install(package: Package) -> eyre::Result<()> {
        let mut tar = Vec::new();

        let mut gz = flate2::read::GzDecoder::new(package.tgz.reader());

        gz.read_to_end(&mut tar)
            .wrap_err("Failed to decompress package")?;

        let mut tar = tar::Archive::new(Bytes::from(tar).reader());

        let pkg_dir = Path::new(Self::PROTO_DEP_PATH).join(package.manifest.name.as_package_dir());

        Self::uninstall(&package.manifest.name).await.ok();
        fs::create_dir_all(&pkg_dir)
            .await
            .wrap_err("Failed to install dependencies")?;

        tar.unpack(pkg_dir)
            .wrap_err(format!("Failed to unpack tar of {}", package.manifest.name))?;

        tracing::info!(
            "+ installed {}@{}",
            package.manifest.name,
            package.manifest.version
        );

        Ok(())
    }

    /// Uninstalls a package from the local file system
    pub async fn uninstall(package: &PackageId) -> eyre::Result<()> {
        let pkg_dir = Path::new(Self::PROTO_DEP_PATH).join(package.as_package_dir());

        fs::remove_dir_all(&pkg_dir)
            .await
            .wrap_err("Failed to uninstall {dependency}")
    }

    /// Packages a release from the local file system state
    pub async fn release() -> eyre::Result<Package> {
        let mut manifest = RawManifest::from(Manifest::read().await?);
        manifest.dependencies = None;

        let pkg = manifest
            .package
            .to_owned()
            .wrap_err("Releasing a package requires a package manifest")?;

        let manifest = toml::to_string_pretty(&manifest)
            .wrap_err("Failed to encode release manifest")?
            .as_bytes()
            .to_vec();

        let mut archive = tar::Builder::new(Vec::new());

        let pkg_path = fs::canonicalize(PathBuf::from_str(Self::PROTO_API_PATH)?)
            .await
            .wrap_err("Failed to locate api package")?;

        for entry in WalkDir::new(pkg_path).into_iter().filter_map(|e| e.ok()) {
            let ext = entry
                .path()
                .extension()
                .map(|s| s.to_str())
                .unwrap_or_default()
                .unwrap_or_default();

            if ext != "proto" {
                continue;
            }

            archive
                .append_path_with_name(
                    entry.path(),
                    entry
                        .path()
                        .file_name()
                        .wrap_err("Failed to add protos to release")?,
                )
                .wrap_err("Failed to add protos to release")?;
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

        tracing::info!("+ packaged {}@{}", pkg.name, pkg.version);

        Ok(Package::new(pkg, tgz))
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
    pub fn new(name: PackageId, version: String, tgz: Bytes) -> Self {
        Self { name, version, tgz }
    }
}

/// A `buffrs` package id for parsing and type safety
#[derive(Clone, Hash, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(try_from = "String", into = "String")]
pub struct PackageId(String);

impl PackageId {
    fn as_package_dir(&self) -> String {
        self.0.replace('-', "_")
    }
}

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
