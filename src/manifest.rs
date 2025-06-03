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
    path::{Path, PathBuf},
    str::FromStr,
};
use tokio::fs;

use crate::{
    config::{self},
    errors::{DeserializationError, FileExistsError, SerializationError, WriteError},
    package::{PackageName, PackageType},
    registry::RegistryRef,
    ManagedFile,
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
            self::CANARY_EDITION => Self::Canary,
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
            Self::Canary { dependencies, .. } => dependencies,
            Self::Unknown { dependencies, .. } => dependencies,
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

    impl<'de> Deserialize<'de> for RawManifest {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: Deserializer<'de>,
        {
            static FIELDS: &[&str] = &["package", "dependencies"];

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
                            "dependencies" => {
                                dependencies = Some(map.next_value()?);
                            }
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

                    match Edition::from(edition.as_str()) {
                        Edition::Canary | Edition::Canary09 | Edition::Canary08 | Edition::Canary07 => Ok(RawManifest::Canary {
                            package,
                            dependencies,
                        }),
                        Edition::Unknown => Err(de::Error::custom(
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
            Edition::Canary | Edition::Canary09 | Edition::Canary08 | Edition::Canary07 => {
                RawManifest::Canary {
                    package: manifest.package,
                    dependencies,
                }
            }
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

/// A resolved version of the manifest with registry aliases and local dependencies resolved
pub struct ResolvedManifest(pub Manifest);

impl Manifest {
    /// Create a new manifest of the current edition
    pub fn new(package: Option<PackageManifest>, dependencies: Vec<Dependency>) -> Self {
        Self {
            edition: Edition::latest(),
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
    pub async fn read(config: &config::Config) -> miette::Result<Self> {
        Self::try_read_from(MANIFEST_FILE, Some(config))
            .await?
            .ok_or(miette!("`{MANIFEST_FILE}` does not exist"))
    }

    /// Parses the manifest from the given string
    ///
    /// # Arguments
    /// * `contents` - The contents of the manifest file
    /// * `config` - The configuration to use for resolving registry aliases
    pub fn try_parse(contents: &str, config: Option<&config::Config>) -> miette::Result<Self> {
        let raw: RawManifest = toml::from_str(contents)
            .into_diagnostic()
            .wrap_err(DeserializationError(ManagedFile::Manifest))?;

        let dependencies = raw
            .dependencies()
            .iter()
            .map(|(package, manifest)| {
                let package = package.clone();
                let manifest = match manifest {
                    DependencyManifest::Remote(remote_manifest) => {
                        // For remote manifest dependencies, resolve the registry alias
                        DependencyManifest::Remote(RemoteDependencyManifest {
                            version: remote_manifest.version.clone(),
                            repository: remote_manifest.repository.clone(),
                            registry: remote_manifest.registry.with_alias_resolved(config)?,
                        })
                    }
                    DependencyManifest::Local(local_manifest) => {
                        // For local dependencies, check if a remote manifest is present
                        // and resolve its registry alias
                        if let Some(ref remote_manifest) = local_manifest.publish {
                            DependencyManifest::Local(LocalDependencyManifest {
                                path: local_manifest.path.clone(),
                                publish: Some(RemoteDependencyManifest {
                                    version: remote_manifest.version.clone(),
                                    repository: remote_manifest.repository.clone(),
                                    registry: remote_manifest
                                        .registry
                                        .with_alias_resolved(config)?,
                                }),
                            })
                        } else {
                            manifest.clone()
                        }
                    }
                };

                Ok(Dependency { package, manifest })
            })
            .collect::<miette::Result<Vec<_>>>()?;

        Ok(Self {
            edition: raw.edition(),
            package: raw.package().cloned(),
            dependencies,
        })
    }

    /// Loads the manifest from the given path
    ///
    /// # Arguments
    /// * `path` - The path to the manifest file
    /// * `config` - The configuration to use for resolving registry aliases
    pub async fn try_read_from(
        path: impl AsRef<Path>,
        config: Option<&config::Config>,
    ) -> miette::Result<Option<Self>> {
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

        Ok(Some(Self::try_parse(&contents, config)?))
    }

    /// Persists the manifest into the current directory
    pub async fn write(&self) -> miette::Result<()> {
        self.write_at(Path::new(".")).await
    }

    /// Persists the manifest into the provided directory, which must exist
    pub async fn write_at(&self, dir_path: &Path) -> miette::Result<()> {
        // hint: create a canary manifest from the current one
        let raw = RawManifest::from(Manifest::new(
            self.package.clone(),
            self.dependencies.clone(),
        ));

        let manifest_file_path = dir_path.join(MANIFEST_FILE);
        fs::write(
            manifest_file_path,
            toml::to_string(&raw)
                .into_diagnostic()
                .wrap_err(SerializationError(ManagedFile::Manifest))?
                .into_bytes(),
        )
        .await
        .into_diagnostic()
        .wrap_err(WriteError(MANIFEST_FILE))
    }

    /// Tests if the manifest is fully resolved (only contains remote dependencies)
    pub fn assert_fully_resolved(&self) -> miette::Result<()> {
        for dependency in &self.dependencies {
            if dependency.manifest.is_local() {
                return Err(miette!(
                    "dependency {} of {} does not specify version/registry/repository",
                    dependency.package,
                    self.package
                        .as_ref()
                        .map(|p| p.name.clone())
                        .map(|n| n.to_string())
                        .unwrap_or("package".to_string())
                ));
            }
        }

        Ok(())
    }
}

impl ResolvedManifest {
    /// Returns a clone of this manifest suitable for publishing
    ///
    /// - All local manifest dependencies are replaced with their remote counterparts
    pub fn new_from_manifest(mut manifest: Manifest) -> miette::Result<Self> {
        // Resolve aliases in dependencies prior to packaging
        for dependency in &mut manifest.dependencies {
            if let DependencyManifest::Local(ref local_manifest) = dependency.manifest {
                match local_manifest.publish {
                    Some(ref remote_manifest) => {
                        dependency.manifest = DependencyManifest::Remote(remote_manifest.clone());
                    }
                    None => {
                        return Err(miette!(
                            "local dependency {} of {} does not specify version/registry/repository",
                            dependency.package,
                            manifest.package
                                .as_ref()
                                .map(|p| p.name.clone())
                                .map(|n| n.to_string())
                                .unwrap_or("package".to_string())
                        ));
                    }
                }
            }
        }

        Ok(Self(manifest))
    }
}

/// Trait for serializable manifest types
pub trait PublishableManifest {
    /// Returns the header for the manifest file
    fn header() -> Option<&'static str>;
    /// Returns the name of the manifest file
    fn file_name() -> String;
}

impl PublishableManifest for Manifest {
    fn header() -> Option<&'static str> {
        None
    }

    fn file_name() -> String {
        format!("{}.orig", MANIFEST_FILE)
    }
}

impl PublishableManifest for ResolvedManifest {
    fn header() -> Option<&'static str> {
        const MANIFEST_PREFIX: &str = r#"# THIS FILE IS AUTOMATICALLY GENERATED BY BUFFRS
#
# When uploading packages to the registry buffrs will automatically
# "normalize" Proto.toml files for maximal compatibility
# with all versions of buffrs and also rewrite `path` dependencies
# to registry dependencies.
#
# If you are reading this file be aware that the original Proto.toml
# will likely look very different (and much more reasonable).
# See Proto.toml.orig for the original contents.
"#;

        Some(MANIFEST_PREFIX)
    }

    fn file_name() -> String {
        MANIFEST_FILE.to_owned()
    }
}

impl TryInto<ResolvedManifest> for Manifest {
    type Error = miette::Report;

    fn try_into(self) -> Result<ResolvedManifest, Self::Error> {
        ResolvedManifest::new_from_manifest(self)
    }
}

impl TryInto<String> for Manifest {
    type Error = toml::ser::Error;

    fn try_into(self) -> Result<String, Self::Error> {
        toml::to_string_pretty(&RawManifest::from(self))
    }
}

impl TryInto<String> for ResolvedManifest {
    type Error = toml::ser::Error;

    fn try_into(self) -> Result<String, Self::Error> {
        self.0.try_into()
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
        registry: &RegistryRef,
        repository: String,
        package: PackageName,
        version: VersionReq,
    ) -> Self {
        Self {
            package,
            manifest: RemoteDependencyManifest {
                repository,
                version,
                registry: registry.to_owned(),
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
#[derive(Debug, Clone, Hash, Serialize, PartialEq, Eq)]
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
    pub registry: RegistryRef,
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
    /// Optional remote manifest for publishing
    #[serde(flatten)]
    pub publish: Option<RemoteDependencyManifest>,
}

impl From<LocalDependencyManifest> for DependencyManifest {
    fn from(value: LocalDependencyManifest) -> Self {
        Self::Local(value)
    }
}

// Custom deserialization logic for `DependencyManifest`
mod dependency_manifest_deserializer {
    use super::*;
    use serde::{de::Error, Deserialize, Deserializer};

    impl<'de> Deserialize<'de> for DependencyManifest {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: Deserializer<'de>,
        {
            #[derive(Deserialize)]
            struct TempManifest {
                path: Option<PathBuf>,
                version: Option<VersionReq>,
                repository: Option<String>,
                registry: Option<RegistryRef>,
            }

            let temp: TempManifest = TempManifest::deserialize(deserializer)?;

            if let Some(path) = temp.path {
                // Deserialize as a local dependency with optional remote attributes
                Ok(DependencyManifest::Local(LocalDependencyManifest {
                    path,
                    publish: match (temp.version, temp.repository, temp.registry) {
                        (Some(version), Some(repository), Some(registry)) => {
                            Some(RemoteDependencyManifest {
                                version,
                                repository,
                                registry,
                            })
                        }
                        _ => None,
                    },
                }))
            } else if let (Some(version), Some(repository), Some(registry)) =
                (temp.version, temp.repository, temp.registry)
            {
                // Deserialize as a remote dependency
                Ok(DependencyManifest::Remote(RemoteDependencyManifest {
                    version,
                    repository,
                    registry,
                }))
            } else {
                Err(D::Error::custom("Invalid dependency manifest"))
            }
        }
    }
}
