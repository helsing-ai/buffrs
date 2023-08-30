// (c) Copyright 2023 Helsing GmbH. All rights reserved.

use std::path::Path;
use std::{collections::HashMap, fmt};

use eyre::Context;
use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};
use tokio::fs;

use crate::package::{PackageId, PackageType};

// TODO(rfink): For symmetry between the vendored and the local packages, we should consider
//  putting the manifest file into proto/Proto.toml .
pub const MANIFEST_FILE: &str = "Proto.toml";

/// A `buffrs` manifest format used for serialization and deserialization.
///
/// This contains the exact structure of the `Proto.toml` and skips
/// empty fields.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RawManifest {
    pub package: Option<PackageManifest>,
    pub dependencies: Option<DependencyMap>,
}

impl From<Manifest> for RawManifest {
    fn from(manifest: Manifest) -> Self {
        let dependencies: DependencyMap = manifest
            .dependencies
            .iter()
            .map(|dep| (dep.package.to_owned(), dep.manifest.to_owned()))
            .collect();

        let dependencies = (!dependencies.is_empty()).then_some(dependencies);

        Self {
            package: manifest.package,
            dependencies,
        }
    }
}

/// Map representation of the dependency list
pub type DependencyMap = HashMap<PackageId, DependencyManifest>;

/// The `buffrs` manifest format used for internal processing, contains a parsed
/// version of the `RawManifest` for easier use.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Manifest {
    pub package: Option<PackageManifest>,
    pub dependencies: Vec<Dependency>,
}

impl Manifest {
    pub async fn exists() -> eyre::Result<bool> {
        fs::try_exists(MANIFEST_FILE)
            .await
            .wrap_err("Failed to detect manifest")
    }

    pub async fn read(base_dir: &Path) -> eyre::Result<Self> {
        let toml = fs::read_to_string(base_dir.join(MANIFEST_FILE))
            .await
            .wrap_err("Failed to read manifest")?;

        let raw: RawManifest = toml::from_str(&toml).wrap_err("Failed to parse manifest")?;

        Ok(raw.into())
    }

    pub async fn write(&self, base_dir: &Path) -> eyre::Result<()> {
        let raw = RawManifest::from(self.to_owned());

        fs::write(
            base_dir.join(MANIFEST_FILE),
            toml::to_string(&raw)?.into_bytes(),
        )
        .await
        .wrap_err("Failed to write manifest")
    }
}

impl From<RawManifest> for Manifest {
    fn from(raw: RawManifest) -> Self {
        let dependencies = raw
            .dependencies
            .unwrap_or_default()
            .iter()
            .map(|(package, manifest)| Dependency {
                package: package.to_owned(),
                manifest: manifest.to_owned(),
            })
            .collect();

        Self {
            package: raw.package,
            dependencies,
        }
    }
}

/// Manifest format for api packages
#[derive(Debug, Clone, Hash, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct PackageManifest {
    /// Type of the package
    pub r#type: PackageType,
    /// Name of the package
    pub name: PackageId,
    /// Version of the package
    pub version: Version,
    /// Description of the api package
    pub description: Option<String>,
}

/// Represents a single project dependency
#[derive(Clone, Debug, Hash, Serialize, Deserialize, PartialEq, Eq)]
pub struct Dependency {
    /// Package name of this dependency
    pub package: PackageId,
    /// Version requirement in the buffrs format, currently only supports pinning
    pub manifest: DependencyManifest,
}

impl Dependency {
    /// Creates a new dependency
    pub fn new(repository: String, package: PackageId, version: VersionReq) -> Self {
        Self {
            package,
            manifest: DependencyManifest {
                repository,
                version,
            },
        }
    }
}

impl fmt::Display for Dependency {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}/{}@{}",
            self.manifest.repository, self.package, self.manifest.version
        )
    }
}

/// Manifest format for dependencies
#[derive(Debug, Clone, Hash, Serialize, Deserialize, PartialEq, Eq)]
pub struct DependencyManifest {
    /// Version requirement in the buffrs format, currently only supports pinning
    pub version: VersionReq,
    /// Artifactory repository to pull dependency from
    pub repository: String,
}
