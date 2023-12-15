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

use miette::{miette, Context, IntoDiagnostic};
use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fmt::{self, Display},
    path::Path,
    str::FromStr,
};
use tokio::fs;

use crate::{
    errors::{DeserializationError, FileExistsError, SerializationError, WriteError},
    package::{PackageName, PackageType},
    registry::RegistryUri,
    ManagedFile,
};

/// The name of the manifest file
pub const MANIFEST_FILE: &str = "Proto.toml";

/// The canary edition supported by this version of buffrs
pub const CANARY_EDITION: &str = concat!("0.", env!("CARGO_PKG_VERSION_MINOR"));

/// Edition of the buffrs manifest
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Edition {
    /// The canary edition of manifests
    ///
    /// This indicates that breaking changes and unstable behavior can occur
    /// at any time. Users are responsible for consulting documentation and
    /// help channels if errors occur.
    Canary,
    /// Unknown edition of manifests
    ///
    /// This is unrecommended as breaking changes could be introduced due to being
    /// in the beta release channel
    Unknown,
}

impl Default for Edition {
    fn default() -> Self {
        Self::Canary
    }
}

/// A buffrs manifest format used for serialization and deserialization.
///
/// This contains the exact structure of the `Proto.toml` and skips
/// empty fields.
#[derive(Debug, Clone, PartialEq, Eq)]
enum RawManifest {
    Canary {
        package: Option<PackageManifest>,
        dependencies: DependencyMap,
    },
    Unknown {
        package: Option<PackageManifest>,
        dependencies: DependencyMap,
    },
}

impl RawManifest {
    fn package(&self) -> Option<&PackageManifest> {
        match self {
            Self::Canary { package, .. } => package.as_ref(),
            Self::Unknown { package, .. } => package.as_ref(),
        }
    }

    fn dependencies(&self) -> &DependencyMap {
        match self {
            Self::Canary { dependencies, .. } => &dependencies,
            Self::Unknown { dependencies, .. } => &dependencies,
        }
    }

    fn edition(&self) -> Edition {
        match self {
            Self::Canary { .. } => Edition::Canary,
            Self::Unknown { .. } => Edition::Unknown,
        }
    }
}

mod serializer {
    use super::*;
    use serde::{ser::SerializeStruct, Serializer};

    impl Serialize for Edition {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            match self {
                Self::Canary => serializer.serialize_str(CANARY_EDITION),
                Self::Unknown => serializer.serialize_str("unknown"),
            }
        }
    }

    impl Serialize for RawManifest {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            match *self {
                RawManifest::Canary {
                    ref package,
                    ref dependencies,
                } => {
                    let mut s = serializer.serialize_struct("Canary", 3)?;
                    s.serialize_field("edition", CANARY_EDITION)?;
                    s.serialize_field("package", package)?;
                    s.serialize_field("dependencies", dependencies)?;
                    s.end()
                }
                RawManifest::Unknown {
                    ref package,
                    ref dependencies,
                } => {
                    let mut s = serializer.serialize_struct("Unknown", 2)?;
                    s.serialize_field("package", package)?;
                    s.serialize_field("dependencies", dependencies)?;
                    s.end()
                }
            }
        }
    }
}

mod deserializer {
    use serde::{
        de::{self, MapAccess, Visitor},
        Deserializer,
    };

    use super::*;

    impl<'de> Deserialize<'de> for Edition {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: Deserializer<'de>,
        {
            struct EditionVisitor;

            impl<'de> serde::de::Visitor<'de> for EditionVisitor {
                type Value = Edition;

                fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                    formatter.write_str("`Alpha` or `Beta`")
                }

                fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
                where
                    E: serde::de::Error,
                {
                    match value {
                        c if c == CANARY_EDITION => Ok(Edition::Canary),
                        _ => Ok(Edition::Unknown),
                    }
                }
            }

            deserializer.deserialize_str(EditionVisitor)
        }
    }

    impl<'de> Deserialize<'de> for RawManifest {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: Deserializer<'de>,
        {
            static FIELDS: &'static [&'static str] = &["package", "dependencies"];

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

                    while let Some(key) = map.next_key::<String>()? {
                        match key.as_str() {
                            "package" => package = Some(map.next_value()?),
                            "dependencies" => dependencies = Some(map.next_value()?),
                            "edition" => edition = Some(map.next_value()?),
                            _ => return Err(de::Error::unknown_field(&key, FIELDS)),
                        }
                    }

                    let dependencies = dependencies.unwrap_or_default();

                    let Some(edition) = edition else {
                        return Ok(RawManifest::Unknown {
                            package,
                            dependencies,
                        });
                    };

                    let edition = serde_typename::from_str(&edition);

                    match edition {
                        Ok(Edition::Canary) => Ok(RawManifest::Canary {
                            package,
                            dependencies,
                        }),
                        Ok(Edition::Unknown) | Err(_) => Err(de::Error::custom(
                            format!("unsupported manifest edition, supported editions of {} are: {CANARY_EDITION}", env!("CARGO_PKG_VERSION"))
                        )),
                    }
                }
            }

            deserializer.deserialize_map(ManifestVisitor)
        }
    }
}

impl From<Manifest> for RawManifest {
    fn from(manifest: Manifest) -> Self {
        let dependencies: DependencyMap = manifest
            .dependencies
            .into_iter()
            .map(|dep| (dep.package, dep.manifest))
            .collect();

        match manifest.edition {
            Edition::Canary => RawManifest::Canary {
                package: manifest.package,
                dependencies,
            },
            Edition::Unknown => RawManifest::Unknown {
                package: manifest.package,
                dependencies,
            },
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

/// The buffrs manifest format used for internal processing, contains a parsed
/// version of the `RawManifest` for easier use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    /// Edition of this manifest
    pub edition: Edition,
    /// Metadata about the root package
    pub package: Option<PackageManifest>,
    /// List of packages the root package depends on
    pub dependencies: Vec<Dependency>,
}

impl Manifest {
    /// Create a new manifest of the current edition
    pub fn new(package: Option<PackageManifest>, dependencies: Vec<Dependency>) -> Self {
        Self {
            edition: Edition::default(),
            package,
            dependencies,
        }
    }

    /// Checks if the manifest file exists in the filesystem
    pub async fn exists() -> miette::Result<bool> {
        fs::try_exists(MANIFEST_FILE)
            .await
            .into_diagnostic()
            .wrap_err(FileExistsError(MANIFEST_FILE))
    }

    /// Loads the manifest from the current directory
    pub async fn read() -> miette::Result<Self> {
        Self::try_read_from(MANIFEST_FILE)
            .await?
            .ok_or(miette!("`{MANIFEST_FILE}` does not exist"))
    }

    /// Loads the manifest from the given path
    pub async fn try_read_from(path: impl AsRef<Path>) -> miette::Result<Option<Self>> {
        let contents = match fs::read_to_string(path.as_ref()).await {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(None);
            }
            Err(e) => {
                return Err(e).into_diagnostic().wrap_err(miette!(
                    "failed to read manifest from `{}`",
                    path.as_ref().display()
                ));
            }
        };

        let raw: RawManifest = toml::from_str(&contents)
            .into_diagnostic()
            .wrap_err(DeserializationError(ManagedFile::Manifest))?;

        Ok(Some(raw.into()))
    }

    /// Persists the manifest into the current directory
    pub async fn write(&self) -> miette::Result<()> {
        // hint: create a canary manifest from the current one
        let raw = RawManifest::from(Manifest::new(
            self.package.clone(),
            self.dependencies.clone(),
        ));

        fs::write(
            MANIFEST_FILE,
            toml::to_string(&raw)
                .into_diagnostic()
                .wrap_err(SerializationError(ManagedFile::Manifest))?
                .into_bytes(),
        )
        .await
        .into_diagnostic()
        .wrap_err(WriteError(MANIFEST_FILE))
    }
}

impl From<RawManifest> for Manifest {
    fn from(raw: RawManifest) -> Self {
        let dependencies = raw
            .dependencies()
            .iter()
            .map(|(package, manifest)| Dependency {
                package: package.to_owned(),
                manifest: manifest.to_owned(),
            })
            .collect();

        Self {
            edition: raw.edition(),
            package: raw.package().cloned(),
            dependencies,
        }
    }
}

impl FromStr for Manifest {
    type Err = toml::de::Error;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        input.parse::<RawManifest>().map(Self::from)
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
            manifest: DependencyManifest {
                repository,
                version,
                registry,
            },
        }
    }

    /// Creates a copy of this dependency with a pinned version
    pub fn with_version(&self, version: &Version) -> Dependency {
        let mut dependency = self.clone();
        dependency.manifest.version = VersionReq {
            comparators: vec![semver::Comparator {
                op: semver::Op::Exact,
                major: version.major,
                minor: Some(version.minor),
                patch: Some(version.patch),
                pre: version.pre.clone(),
            }],
        };
        dependency
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
