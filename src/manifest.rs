// (c) Copyright 2023 Helsing GmbH. All rights reserved.

use eyre::{ensure, Context};
use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fmt::{self, Display},
    ops::DerefMut,
    str::FromStr,
};
use tokio::fs;
use url::Url;

use crate::package::{PackageId, PackageType};

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
            .into_iter()
            .map(|dep| (dep.package, dep.manifest))
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

    pub async fn read() -> eyre::Result<Self> {
        let toml = fs::read_to_string(MANIFEST_FILE)
            .await
            .wrap_err("Failed to read manifest")?;

        let raw: RawManifest = toml::from_str(&toml).wrap_err("Failed to parse manifest")?;

        Ok(raw.into())
    }

    pub async fn write(&self) -> eyre::Result<()> {
        let raw = RawManifest::from(self.to_owned());

        fs::write(MANIFEST_FILE, toml::to_string(&raw)?.into_bytes())
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
    pub fn new(
        registry: RegistryUri,
        repository: String,
        package: PackageId,
        version: VersionReq,
    ) -> Self {
        Self {
            package,
            manifest: DependencyManifest {
                repository,
                version,
                registry,
            },
        }
    }
}

impl Display for Dependency {
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
    /// Artifactory registry to pull from
    pub registry: RegistryUri,
}

#[derive(Debug, Clone, Hash, Serialize, Deserialize, PartialEq, Eq)]
pub struct RegistryUri(Url);

impl std::ops::Deref for RegistryUri {
    type Target = Url;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for RegistryUri {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl Display for RegistryUri {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for RegistryUri {
    type Err = eyre::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let slf = Self(Url::from_str(value)?);
        slf.sanity_check_url()?;
        Ok(slf)
    }
}

impl RegistryUri {
    pub fn sanity_check_url(&self) -> eyre::Result<()> {
        tracing::debug!(
            "checking that url begins with http or https: {}",
            self.0.scheme()
        );
        ensure!(
            self.0.scheme() == "http" || self.0.scheme() == "https",
            "The self.0 must start with http:// or https://"
        );

        if let Some(host) = self.0.host_str() {
            if host.ends_with(".jfrog.io") {
                tracing::debug!(
                    "checking that jfrog.io url ends with /artifactory: {}",
                    self.0.path()
                );
                ensure!(
                    self.0.path().ends_with("/artifactory"),
                    "The url must end with '/artifactory' when using a *.jfrog.io host"
                );
            }
        }

        Ok(())
    }
}
