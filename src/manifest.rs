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

use eyre::Context;
use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, fmt};
use tokio::fs;

use crate::package::{PackageName, PackageType};

pub const MANIFEST_FILE: &str = "Proto.toml";

/// A `buffrs` manifest format used for serialization and deserialization.
///
/// This contains the exact structure of the `Proto.toml` and skips
/// empty fields.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct RawManifest {
    package: PackageManifest,
    dependencies: DependencyMap,
}

impl From<Manifest> for RawManifest {
    fn from(manifest: Manifest) -> Self {
        let dependencies: DependencyMap = manifest
            .dependencies
            .into_iter()
            .map(|dep| (dep.package, dep.manifest))
            .collect();

        Self {
            package: manifest.package,
            dependencies,
        }
    }
}

impl TryFrom<String> for RawManifest {
    type Error = toml::de::Error;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        toml::from_str::<RawManifest>(&value)
    }
}

/// Map representation of the dependency list
pub type DependencyMap = HashMap<PackageName, DependencyManifest>;

/// The `buffrs` manifest format used for internal processing, contains a parsed
/// version of the `RawManifest` for easier use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    /// Metadata about the root package
    pub package: PackageManifest,
    /// List of packages the root package depends on
    pub dependencies: Vec<Dependency>,
}

impl Manifest {
    /// Checks if the manifest file exists in the filesystem
    pub async fn exists() -> eyre::Result<bool> {
        fs::try_exists(MANIFEST_FILE)
            .await
            .wrap_err("Failed to detect manifest")
    }

    /// Loads the manifest from the current directory
    pub async fn read() -> eyre::Result<Self> {
        let toml = fs::read_to_string(MANIFEST_FILE)
            .await
            .wrap_err("Failed to read manifest")?;

        let raw: RawManifest = toml::from_str(&toml).wrap_err("Failed to parse manifest")?;

        Ok(raw.into())
    }

    /// Persists the manifest into the current directory
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

impl TryFrom<String> for Manifest {
    type Error = toml::de::Error;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Ok(RawManifest::try_from(value)?.into())
    }
}

impl TryInto<String> for Manifest {
    type Error = toml::ser::Error;

    fn try_into(self) -> Result<String, Self::Error> {
        toml::to_string_pretty(&RawManifest::from(self))
    }
}

/// Manifest format for api packages
#[derive(Debug, Clone, Hash, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct PackageManifest {
    /// Type of the package
    pub r#type: PackageType,
    /// Name of the package
    pub name: PackageName,
    /// Version of the package
    pub version: Version,
    /// Description of the api package
    pub description: Option<String>,
}

/// Represents a single project dependency
#[derive(Clone, Debug, Hash, Serialize, Deserialize, PartialEq, Eq)]
pub struct Dependency {
    /// Package name of this dependency
    pub package: PackageName,
    /// Version requirement in the buffrs format, currently only supports pinning
    pub manifest: DependencyManifest,
}

impl Dependency {
    /// Creates a new dependency
    pub fn new(repository: String, package: PackageName, version: VersionReq) -> Self {
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
