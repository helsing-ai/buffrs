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
    fmt::{self, Display},
    path::PathBuf,
    str::FromStr,
};

use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};

use crate::{
    package::{PackageName, PackageType},
    registry::RegistryUri,
    workspace::Workspace,
};

/// The name of the manifest file
pub const MANIFEST_FILE: &str = "Proto.toml";

/// The canary edition supported by this version of buffrs
pub const CANARY_EDITION: &str = concat!("0.", env!("CARGO_PKG_VERSION_MINOR"));

/// Edition of the buffrs manifest
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(into = "&str", from = "&str")]
pub enum Edition {
    /// The canary edition of manifests
    ///
    /// This indicates that breaking changes and unstable behavior can occur
    /// at any time. Users are responsible for consulting documentation and
    /// help channels if errors occur.
    Canary,
    /// The canary edition used by buffrs 0.11.x
    Canary11,
    /// The canary edition used by buffrs 0.10.x
    Canary10,
    /// The canary edition used by buffrs 0.9.x
    Canary09,
    /// The canary edition used by buffrs 0.8.x
    Canary08,
    /// The canary edition used by buffrs 0.7.x
    Canary07,
    /// Unknown edition of manifests
    ///
    /// This is unrecommended as breaking changes could be introduced due to being
    /// in the beta release channel
    Unknown,
}

impl Edition {
    /// The current / latest edition of buffrs
    pub fn latest() -> Self {
        Self::Canary
    }
}

impl From<&str> for Edition {
    fn from(value: &str) -> Self {
        match value {
            CANARY_EDITION => Self::Canary,
            "0.11" => Self::Canary11,
            "0.10" => Self::Canary10,
            "0.9" => Self::Canary09,
            "0.8" => Self::Canary08,
            "0.7" => Self::Canary07,
            _ => Self::Unknown,
        }
    }
}

impl From<Edition> for &'static str {
    fn from(value: Edition) -> Self {
        match value {
            Edition::Canary => CANARY_EDITION,
            Edition::Canary11 => "0.11",
            Edition::Canary10 => "0.10",
            Edition::Canary09 => "0.9",
            Edition::Canary08 => "0.8",
            Edition::Canary07 => "0.7",
            Edition::Unknown => "unknown",
        }
    }
}

/// A buffrs manifest format used for serialization and deserialization.
///
/// This contains the exact structure of the `Proto.toml` and skips
/// empty fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RawManifest {
    /// A raw manifest with a canary version
    Canary {
        /// The optional package manifest
        package: Option<PackageManifest>,
        /// The optional dependencies
        dependencies: Option<DependencyMap>,
        /// The optional workspace
        workspace: Option<Workspace>,
    },
    /// A raw manifest with an unknown canary
    Unknown {
        /// The optional package manifest
        package: Option<PackageManifest>,
        /// The optional dependencies
        dependencies: Option<DependencyMap>,
        /// The optional workspace
        workspace: Option<Workspace>,
    },
}

impl RawManifest {
    pub(crate) fn package(&self) -> Option<&PackageManifest> {
        match self {
            Self::Canary { package, .. } => package.as_ref(),
            Self::Unknown { package, .. } => package.as_ref(),
        }
    }

    pub(crate) fn dependencies(&self) -> Option<&DependencyMap> {
        match self {
            Self::Canary { dependencies, .. } => dependencies.as_ref(),
            Self::Unknown { dependencies, .. } => dependencies.as_ref(),
        }
    }

    pub(crate) fn dependencies_as_vec(&self) -> Option<Vec<Dependency>> {
        self.dependencies().map(|deps| {
            deps.iter()
                .map(|(package, manifest)| Dependency {
                    package: package.to_owned(),
                    manifest: manifest.to_owned(),
                })
                .collect()
        })
    }

    pub(crate) fn edition(&self) -> Edition {
        match self {
            Self::Canary { .. } => Edition::Canary,
            Self::Unknown { .. } => Edition::Unknown,
        }
    }

    pub(crate) fn workspace(&self) -> Option<&Workspace> {
        match self {
            Self::Canary { workspace, .. } => workspace.as_ref(),
            Self::Unknown { workspace, .. } => workspace.as_ref(),
        }
    }
}

mod serializer {
    use super::*;
    use serde::{Serializer, ser::SerializeStruct};

    impl Serialize for RawManifest {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            match *self {
                RawManifest::Canary {
                    ref package,
                    ref dependencies,
                    ref workspace,
                } => {
                    let mut s = serializer.serialize_struct("Canary", 3)?;
                    s.serialize_field("edition", CANARY_EDITION)?;
                    s.serialize_field("package", package)?;
                    s.serialize_field("dependencies", dependencies)?;
                    s.serialize_field("workspace", workspace)?;
                    s.end()
                }
                RawManifest::Unknown {
                    ref package,
                    ref dependencies,
                    ref workspace,
                } => {
                    let mut s = serializer.serialize_struct("Unknown", 2)?;
                    s.serialize_field("package", package)?;
                    s.serialize_field("dependencies", dependencies)?;
                    s.serialize_field("workspace", workspace)?;
                    s.end()
                }
            }
        }
    }
}

mod deserializer {
    use serde::{
        Deserializer,
        de::{self, MapAccess, Visitor},
    };

    use super::*;

    impl<'de> Deserialize<'de> for RawManifest {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: Deserializer<'de>,
        {
            static FIELDS: &[&str] = &["package", "dependencies", "workspace"];

            struct ManifestVisitor;

            impl<'de> Visitor<'de> for ManifestVisitor {
                type Value = RawManifest;

                fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                    formatter.write_str("a buffrs manifest (`Proto.toml`)")
                }

                fn visit_map<V>(self, mut map: V) -> Result<RawManifest, V::Error>
                where
                    V: MapAccess<'de>,
                {
                    let mut edition: Option<String> = None;
                    let mut package: Option<PackageManifest> = None;
                    let mut dependencies: Option<HashMap<PackageName, DependencyManifest>> = None;
                    let mut workspace: Option<Workspace> = None;

                    while let Some(key) = map.next_key::<String>()? {
                        match key.as_str() {
                            "package" => package = Some(map.next_value()?),
                            "dependencies" => dependencies = Some(map.next_value()?),
                            "edition" => edition = Some(map.next_value()?),
                            "workspace" => workspace = Some(map.next_value()?),
                            _ => return Err(de::Error::unknown_field(&key, FIELDS)),
                        }
                    }

                    let Some(edition) = edition else {
                        return Ok(RawManifest::Unknown {
                            package,
                            dependencies,
                            workspace,
                        });
                    };

                    match Edition::from(edition.as_str()) {
                        Edition::Canary
                        | Edition::Canary11
                        | Edition::Canary10
                        | Edition::Canary09
                        | Edition::Canary08
                        | Edition::Canary07 => Ok(RawManifest::Canary {
                            package,
                            dependencies,
                            workspace,
                        }),
                        Edition::Unknown => Err(de::Error::custom(format!(
                            "unsupported manifest edition, supported editions of {} are: {CANARY_EDITION}",
                            env!("CARGO_PKG_VERSION")
                        ))),
                    }
                }
            }

            deserializer.deserialize_map(ManifestVisitor)
        }
    }
}

impl FromStr for RawManifest {
    type Err = toml::de::Error;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        toml::from_str(input)
    }
}

/// Map representation of the dependency list
pub type DependencyMap = HashMap<PackageName, DependencyManifest>;

/// A manifest can define either a package or a workspace
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManifestType {
    /// The Manifest defines a package
    Package,
    /// The Manifest defines a workspace
    Workspace,
}

/// Manifest format for api packages
#[derive(Debug, Clone, Hash, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct PackageManifest {
    /// Type of the package
    #[serde(rename = "type")]
    pub kind: PackageType,
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
    pub fn new(
        registry: RegistryUri,
        repository: String,
        package: PackageName,
        version: VersionReq,
    ) -> Self {
        Self {
            package,
            manifest: RemoteDependencyManifest {
                repository,
                version,
                registry,
            }
            .into(),
        }
    }

    /// Creates a copy of this dependency with a pinned version
    pub fn with_version(&self, version: &Version) -> Dependency {
        let mut dependency = self.clone();

        if let DependencyManifest::Remote(ref mut manifest) = dependency.manifest {
            manifest.version = VersionReq {
                comparators: vec![semver::Comparator {
                    op: semver::Op::Exact,
                    major: version.major,
                    minor: Some(version.minor),
                    patch: Some(version.patch),
                    pre: version.pre.clone(),
                }],
            };
        }

        dependency
    }
}

impl Display for Dependency {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.manifest {
            DependencyManifest::Remote(manifest) => write!(
                f,
                "{}/{}@{}",
                manifest.repository, self.package, manifest.version
            ),
            DependencyManifest::Local(manifest) => {
                write!(f, "{}@{}", self.package, manifest.path.display())
            }
        }
    }
}

/// Manifest format for dependencies
#[derive(Debug, Clone, Hash, Serialize, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum DependencyManifest {
    /// A remote dependency from artifactory
    Remote(RemoteDependencyManifest),
    /// A local dependency located on the filesystem
    Local(LocalDependencyManifest),
}

impl DependencyManifest {
    pub(crate) fn is_local(&self) -> bool {
        matches!(self, DependencyManifest::Local(_))
    }
}

/// Manifest format for dependencies
#[derive(Debug, Clone, Hash, Serialize, Deserialize, PartialEq, Eq)]
pub struct RemoteDependencyManifest {
    /// Version requirement in the buffrs format, currently only supports pinning
    pub version: VersionReq,
    /// Artifactory repository to pull dependency from
    pub repository: String,
    /// Artifactory registry to pull from
    pub registry: RegistryUri,
}

impl From<RemoteDependencyManifest> for DependencyManifest {
    fn from(value: RemoteDependencyManifest) -> Self {
        Self::Remote(value)
    }
}

/// Manifest format for local filesystem dependencies
#[derive(Debug, Clone, Hash, Serialize, Deserialize, PartialEq, Eq)]
pub struct LocalDependencyManifest {
    /// Path to local buffrs package
    pub path: PathBuf,
}

impl From<LocalDependencyManifest> for DependencyManifest {
    fn from(value: LocalDependencyManifest) -> Self {
        Self::Local(value)
    }
}
