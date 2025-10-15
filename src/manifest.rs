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

use miette::{Context, IntoDiagnostic, miette};
use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fmt::{self, Display},
    path::{Path, PathBuf},
    str::FromStr,
};
use tokio::fs;

use crate::errors::InvalidManifestError;
use crate::workspace::Workspace;
use crate::{
    ManagedFile,
    errors::{DeserializationError, FileExistsError, SerializationError, WriteError},
    package::{PackageName, PackageType},
    registry::RegistryUri,
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
enum RawManifest {
    Canary {
        package: Option<PackageManifest>,
        dependencies: Option<DependencyMap>,
        workspace: Option<Workspace>,
    },
    Unknown {
        package: Option<PackageManifest>,
        dependencies: Option<DependencyMap>,
        workspace: Option<Workspace>,
    },
}

impl RawManifest {
    fn package(&self) -> Option<&PackageManifest> {
        match self {
            Self::Canary { package, .. } => package.as_ref(),
            Self::Unknown { package, .. } => package.as_ref(),
        }
    }

    fn dependencies(&self) -> Option<&DependencyMap> {
        match self {
            Self::Canary { dependencies, .. } => dependencies.as_ref(),
            Self::Unknown { dependencies, .. } => dependencies.as_ref(),
        }
    }

    fn edition(&self) -> Edition {
        match self {
            Self::Canary { .. } => Edition::Canary,
            Self::Unknown { .. } => Edition::Unknown,
        }
    }

    fn workspace(&self) -> Option<&Workspace> {
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

impl From<Manifest> for RawManifest {
    fn from(manifest: Manifest) -> Self {
        let dependencies = manifest.dependencies.map(|deps| {
            deps.into_iter()
                .map(|dep| (dep.package, dep.manifest))
                .collect()
        });

        let workspace: Option<Workspace> = manifest.workspace;

        match manifest.edition {
            Edition::Canary
            | Edition::Canary11
            | Edition::Canary10
            | Edition::Canary09
            | Edition::Canary08
            | Edition::Canary07 => RawManifest::Canary {
                package: manifest.package,
                dependencies,
                workspace,
            },
            Edition::Unknown => RawManifest::Unknown {
                package: manifest.package,
                dependencies,
                workspace,
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

/// A manifest can define either a package or a workspace
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManifestType {
    /// The Manifest defines a package
    Package,
    /// The Manifest defines a workspace
    Workspace,
}
/// The buffrs manifest format used for internal processing, contains a parsed
/// version of the `RawManifest` for easier use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    /// Edition of this manifest
    pub edition: Edition,
    /// Metadata about the root package
    pub package: Option<PackageManifest>,
    /// List of packages the root package depends on
    pub dependencies: Option<Vec<Dependency>>,
    /// Definition of a buffrs workspace
    pub workspace: Option<Workspace>,
    /// Type of the manifest: workspace or package
    pub manifest_type: ManifestType,
}

impl Manifest {
    /// Get package names of dependencies
    pub fn get_dependency_package_names(&self) -> Vec<PackageName> {
        self.dependencies
            .clone()
            .unwrap_or_default()
            .iter()
            .map(|d| d.package.clone())
            .collect()
    }

    /// Determine the ManifestType based on dependencies and workspace
    fn get_manifest_type(
        dependencies: &Option<Vec<Dependency>>,
        workspace: &Option<Workspace>,
    ) -> miette::Result<ManifestType> {
        match (&dependencies, &workspace) {
            (&Some(_), &Some(_)) => Err(miette!(
                "manifest cannot have both dependencies and workspace sections"
            ))
            .wrap_err(InvalidManifestError(ManagedFile::Manifest)),
            (None, None) => Err(miette!(
                "manifest cannot have both dependencies and workspace sections"
            ))
            .wrap_err(InvalidManifestError(ManagedFile::Manifest)),
            (&Some(_), None) => Ok(ManifestType::Package),
            (None, &Some(_)) => Ok(ManifestType::Workspace),
        }
    }

    /// Creates a builder for constructing a Manifest
    pub fn builder() -> ManifestBuilder<Unset> {
        ManifestBuilder {
            package: None,
            state: Unset,
        }
    }
}

/// Typestate marker: neither dependencies nor workspace set
pub struct Unset;
/// Typestate marker: dependencies set
pub struct WithDependencies {
    dependencies: Vec<Dependency>,
}
/// Typestate marker: workspace set
pub struct WithWorkspace {
    workspace: Workspace,
}

/// Builder for constructing a Manifest using the typestate pattern
pub struct ManifestBuilder<State> {
    package: Option<PackageManifest>,
    state: State,
}

impl ManifestBuilder<Unset> {
    /// Sets the package metadata
    pub fn package(mut self, package: PackageManifest) -> Self {
        self.package = Some(package);
        self
    }

    /// Sets the dependencies, transitioning to package manifest
    pub fn dependencies(self, dependencies: Vec<Dependency>) -> ManifestBuilder<WithDependencies> {
        ManifestBuilder {
            package: self.package,
            state: WithDependencies { dependencies },
        }
    }

    /// Sets the workspace configuration, transitioning to workspace manifest
    pub fn workspace(self, workspace: Workspace) -> ManifestBuilder<WithWorkspace> {
        ManifestBuilder {
            package: self.package,
            state: WithWorkspace { workspace },
        }
    }
}

impl ManifestBuilder<WithDependencies> {
    /// Builds a package manifest
    pub fn build(self) -> Manifest {
        Manifest {
            edition: Edition::latest(),
            package: self.package,
            dependencies: Some(self.state.dependencies),
            workspace: None,
            manifest_type: ManifestType::Package,
        }
    }
}

impl ManifestBuilder<WithWorkspace> {
    /// Builds a workspace manifest
    pub fn build(self) -> Manifest {
        Manifest {
            edition: Edition::latest(),
            package: self.package,
            dependencies: None,
            workspace: Some(self.state.workspace),
            manifest_type: ManifestType::Workspace,
        }
    }
}

impl Manifest {
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
            .await
            .wrap_err(miette!("`{MANIFEST_FILE}` does not exist"))
    }

    /// Loads the manifest from the given path
    pub async fn try_read_from(path: impl AsRef<Path>) -> miette::Result<Self> {
        let contents = match fs::read_to_string(path.as_ref()).await {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // let canonical_name = fs::canonicalize(&path).await.into_diagnostic()?;
                // tracing::info!("failed at: {}", canonical_name.to_str().unwrap());
                return Err(e).into_diagnostic().wrap_err(miette!(
                    "failed to read non-existent manifest file from `{}`",
                    path.as_ref().display()
                ));
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

        raw.try_into()
    }

    /// Persists the manifest into the current directory
    pub async fn write(&self) -> miette::Result<()> {
        self.write_at(Path::new(".")).await
    }

    /// Persists the manifest into the provided directory, which must exist
    pub async fn write_at(&self, dir_path: &Path) -> miette::Result<()> {
        // hint: create a canary manifest from the current one by cloning fields
        let raw = RawManifest::from(Manifest {
            edition: Edition::latest(),
            ..self.clone()
        });

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
}

impl TryFrom<RawManifest> for Manifest {
    type Error = miette::Report;

    fn try_from(raw: RawManifest) -> Result<Manifest, Self::Error> {
        let dependencies = raw.dependencies().map(|deps| {
            deps.iter()
                .map(|(package, manifest)| Dependency {
                    package: package.to_owned(),
                    manifest: manifest.to_owned(),
                })
                .collect()
        });

        let workspace = raw.workspace().cloned();

        let manifest_type = Manifest::get_manifest_type(&dependencies, &workspace)?;

        Ok(Self {
            edition: raw.edition(),
            package: raw.package().cloned(),
            dependencies,
            workspace,
            manifest_type,
        })
    }
}

impl FromStr for Manifest {
    type Err = miette::Report;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        input
            .parse::<RawManifest>()
            .map_err(|_| DeserializationError(ManagedFile::Manifest))
            .map(Manifest::try_from)?
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

#[cfg(test)]
mod tests {
    mod raw_manifest_tests {
        use crate::manifest::{Manifest, RawManifest};
        use std::str::FromStr;

        #[test]
        fn test_cloned_manifest_convert_to_exact_same_string() {
            let manifest = r#"
            edition = "0.12"

            [package]
            type = "lib"
            name = "lib"
            version = "0.0.1"

            [dependencies]
            "#;

            let manifest = Manifest::from_str(manifest).expect("should be valid manifest");
            let cloned_raw_manifest_str = toml::to_string(&RawManifest::from(manifest.clone()))
                .expect("should be convertable to str");
            let raw_manifest_str = toml::to_string(&RawManifest::from(manifest))
                .expect("should be convertable to str");

            assert!(cloned_raw_manifest_str.contains("edition"));
            assert_eq!(cloned_raw_manifest_str, raw_manifest_str);
        }
    }
    mod manifest_tests {
        use crate::manifest::{Edition, Manifest, ManifestBuilder, RawManifest};
        use std::str::FromStr;

        #[test]
        fn invalid_mixed_manifest() {
            let mixed_dep_and_workspace = r#"
        [workspace]

        [dependencies]
        "#;
            let manifest = Manifest::from_str(mixed_dep_and_workspace);
            assert!(manifest.is_err());
            let report = manifest.err().unwrap();
            println!("{}", report.to_string());
            assert!(
                report
                    .to_string()
                    .contains("manifest Proto.toml is invalid")
            )
        }

        #[test]
        fn invalid_empty_manifest() {
            let empty_manifest = "";
            let manifest = Manifest::from_str(empty_manifest);
            assert!(manifest.is_err());
        }

        #[test]
        fn manifest_parsing_ok() {
            let manifest = r#"
            edition = "0.12"

            [package]
            type = "lib"
            name = "lib"
            version = "0.0.1"

            [dependencies]
            "#;

            let manifest = Manifest::from_str(manifest).expect("should be valid manifest");
            let package = manifest.clone().package.expect("should have valid package");

            assert_eq!(manifest.edition, Edition::Canary);
            assert!(manifest.workspace.is_none());

            let manifest_clone = manifest.clone();
            assert_eq!(manifest.edition, manifest_clone.edition);
        }

        /// TODO(mz): Clarify correct behavior for reserialization of manifests
        #[test]
        fn test_add_edition_attribute() {
            let manifest = r#"
            [package]
            type = "lib"
            name = "lib"
            version = "0.0.1"

            [dependencies]
            "#;

            let manifest = Manifest::from_str(manifest).expect("should be valid manifest");
            let raw_manifest_str = toml::to_string(&RawManifest::from(manifest))
                .expect("should be convertable to str");

            // assert!(raw_manifest_str.contains("edition"))
        }

        #[test]
        fn workspace_manifest_roundtrip() {
            let manifest_str = r#"
            edition = "0.12"

            [workspace]
            members = ["pkg1", "pkg2"]
            "#;

            let manifest = Manifest::from_str(manifest_str).expect("should parse");
            assert_eq!(
                manifest.manifest_type,
                crate::manifest::ManifestType::Workspace
            );
            assert!(manifest.dependencies.is_none());
            assert!(manifest.workspace.is_some());

            let serialized: String = manifest.try_into().expect("should serialize");
            assert!(serialized.contains("edition"));
            assert!(serialized.contains("[workspace]"));
        }

        #[test]
        fn builder_enforces_dependencies_xor_workspace() {
            use crate::workspace::Workspace;

            // Package manifest
            let manifest = Manifest::builder().dependencies(vec![]).build();
            assert_eq!(
                manifest.manifest_type,
                crate::manifest::ManifestType::Package
            );

            // Workspace manifest
            let manifest = Manifest::builder()
                .workspace(Workspace {
                    members: None,
                    exclude: None,
                })
                .build();
            assert_eq!(
                manifest.manifest_type,
                crate::manifest::ManifestType::Workspace
            );
        }

        #[test]
        fn unknown_edition_parsed_correctly() {
            let manifest_str = r#"
            edition = "99.99"

            [package]
            type = "lib"
            name = "test"
            version = "0.0.1"

            [dependencies]
            "#;

            let result = Manifest::from_str(manifest_str);
            assert!(result.is_err());
        }

        #[test]
        fn manifest_without_edition_becomes_unknown() {
            let manifest_str = r#"
            [package]
            type = "lib"
            name = "test"
            version = "0.0.1"

            [dependencies]
            "#;

            let manifest = Manifest::from_str(manifest_str).expect("should parse");
            assert_eq!(manifest.edition, Edition::Unknown);
        }
    }

    mod workspace_tests {
        use super::super::*;
        use std::path::PathBuf;

        #[test]
        fn test_resolve_members_with_explicit_members() {
            use std::fs;

            let temp_dir = tempfile::tempdir().unwrap();
            let workspace_root = temp_dir.path();

            // Create member directories with Proto.toml files
            let pkg1 = workspace_root.join("package1");
            let pkg2 = workspace_root.join("package2");
            fs::create_dir(&pkg1).unwrap();
            fs::create_dir(&pkg2).unwrap();
            fs::write(pkg1.join(MANIFEST_FILE), "").unwrap();
            fs::write(pkg2.join(MANIFEST_FILE), "").unwrap();

            let workspace = Workspace {
                members: Some(vec!["package1".to_string(), "package2".to_string()]),
                exclude: None,
            };

            let members = workspace.resolve_members(workspace_root).unwrap();
            assert_eq!(members.len(), 2);
            assert!(members.contains(&PathBuf::from("package1")));
            assert!(members.contains(&PathBuf::from("package2")));
        }

        #[test]
        fn test_resolve_members_with_glob_pattern() {
            use std::fs;

            let temp_dir = tempfile::tempdir().unwrap();
            let workspace_root = temp_dir.path();

            // Create multiple packages at workspace root (1 level deep only)
            let pkg1 = workspace_root.join("pkg1");
            let pkg2 = workspace_root.join("pkg2");
            let lib = workspace_root.join("lib-special");
            fs::create_dir(&pkg1).unwrap();
            fs::create_dir(&pkg2).unwrap();
            fs::create_dir(&lib).unwrap();
            fs::write(pkg1.join(MANIFEST_FILE), "").unwrap();
            fs::write(pkg2.join(MANIFEST_FILE), "").unwrap();
            fs::write(lib.join(MANIFEST_FILE), "").unwrap();

            let workspace = Workspace {
                members: Some(vec!["pkg*".to_string()]),
                exclude: None,
            };

            let members = workspace.resolve_members(workspace_root).unwrap();
            assert_eq!(members.len(), 2);
            assert!(members.contains(&PathBuf::from("pkg1")));
            assert!(members.contains(&PathBuf::from("pkg2")));
            assert!(!members.contains(&PathBuf::from("lib-special")));
        }

        #[test]
        fn test_resolve_members_with_exclude() {
            use std::fs;

            let temp_dir = tempfile::tempdir().unwrap();
            let workspace_root = temp_dir.path();

            // Create packages at workspace root
            let pkg1 = workspace_root.join("pkg1");
            let pkg2 = workspace_root.join("pkg2");
            let internal = workspace_root.join("internal");

            fs::create_dir(&pkg1).unwrap();
            fs::create_dir(&pkg2).unwrap();
            fs::create_dir(&internal).unwrap();

            fs::write(pkg1.join(MANIFEST_FILE), "").unwrap();
            fs::write(pkg2.join(MANIFEST_FILE), "").unwrap();
            fs::write(internal.join(MANIFEST_FILE), "").unwrap();

            let workspace = Workspace {
                members: Some(vec!["*".to_string()]),
                exclude: Some(vec!["internal".to_string()]),
            };

            let members = workspace.resolve_members(workspace_root).unwrap();
            assert_eq!(members.len(), 2);
            assert!(members.contains(&PathBuf::from("pkg1")));
            assert!(members.contains(&PathBuf::from("pkg2")));
            assert!(!members.contains(&PathBuf::from("internal")));
        }

        #[test]
        fn test_resolve_members_with_exclude_glob() {
            use std::fs;

            let temp_dir = tempfile::tempdir().unwrap();
            let workspace_root = temp_dir.path();

            // Create packages at workspace root
            let pkg1 = workspace_root.join("pkg1");
            let pkg2 = workspace_root.join("pkg2");
            let internal1 = workspace_root.join("internal-one");
            let internal2 = workspace_root.join("internal-two");

            fs::create_dir(&pkg1).unwrap();
            fs::create_dir(&pkg2).unwrap();
            fs::create_dir(&internal1).unwrap();
            fs::create_dir(&internal2).unwrap();

            fs::write(pkg1.join(MANIFEST_FILE), "").unwrap();
            fs::write(pkg2.join(MANIFEST_FILE), "").unwrap();
            fs::write(internal1.join(MANIFEST_FILE), "").unwrap();
            fs::write(internal2.join(MANIFEST_FILE), "").unwrap();

            let workspace = Workspace {
                members: Some(vec!["*".to_string()]),
                exclude: Some(vec!["internal*".to_string()]),
            };

            let members = workspace.resolve_members(workspace_root).unwrap();
            assert_eq!(members.len(), 2);
            assert!(members.contains(&PathBuf::from("pkg1")));
            assert!(members.contains(&PathBuf::from("pkg2")));
            assert!(!members.contains(&PathBuf::from("internal-one")));
            assert!(!members.contains(&PathBuf::from("internal-two")));
        }

        #[test]
        fn test_resolve_members_default_star_pattern() {
            use std::fs;

            let temp_dir = tempfile::tempdir().unwrap();
            let workspace_root = temp_dir.path();

            // Create some packages at the root level
            let pkg1 = workspace_root.join("pkg1");
            let pkg2 = workspace_root.join("pkg2");
            fs::create_dir(&pkg1).unwrap();
            fs::create_dir(&pkg2).unwrap();
            fs::write(pkg1.join(MANIFEST_FILE), "").unwrap();
            fs::write(pkg2.join(MANIFEST_FILE), "").unwrap();

            // Workspace with no members specified should default to "*"
            let workspace = Workspace {
                members: None,
                exclude: None,
            };

            let members = workspace.resolve_members(workspace_root).unwrap();
            assert_eq!(members.len(), 2);
            assert!(members.contains(&PathBuf::from("pkg1")));
            assert!(members.contains(&PathBuf::from("pkg2")));
        }

        #[test]
        fn test_resolve_members_ignores_dirs_without_manifest() {
            use std::fs;

            let temp_dir = tempfile::tempdir().unwrap();
            let workspace_root = temp_dir.path();

            // Create directory with Proto.toml
            let pkg1 = workspace_root.join("pkg1");
            fs::create_dir(&pkg1).unwrap();
            fs::write(pkg1.join(MANIFEST_FILE), "").unwrap();

            // Create directory WITHOUT Proto.toml
            let not_a_pkg = workspace_root.join("not-a-package");
            fs::create_dir(&not_a_pkg).unwrap();

            let workspace = Workspace {
                members: Some(vec!["*".to_string()]),
                exclude: None,
            };

            let members = workspace.resolve_members(workspace_root).unwrap();
            assert_eq!(members.len(), 1);
            assert!(members.contains(&PathBuf::from("pkg1")));
            assert!(!members.contains(&PathBuf::from("not-a-package")));
        }

        #[test]
        fn test_resolve_members_mixed_patterns() {
            use std::fs;

            let temp_dir = tempfile::tempdir().unwrap();
            let workspace_root = temp_dir.path();

            // Create packages at workspace root
            let pkg1 = workspace_root.join("pkg1");
            let pkg2 = workspace_root.join("pkg2");
            let special = workspace_root.join("special");

            fs::create_dir(&pkg1).unwrap();
            fs::create_dir(&pkg2).unwrap();
            fs::create_dir(&special).unwrap();
            fs::write(pkg1.join(MANIFEST_FILE), "").unwrap();
            fs::write(pkg2.join(MANIFEST_FILE), "").unwrap();
            fs::write(special.join(MANIFEST_FILE), "").unwrap();

            let workspace = Workspace {
                members: Some(vec!["pkg*".to_string(), "special".to_string()]),
                exclude: None,
            };

            let members = workspace.resolve_members(workspace_root).unwrap();
            assert_eq!(members.len(), 3);
            assert!(members.contains(&PathBuf::from("pkg1")));
            assert!(members.contains(&PathBuf::from("pkg2")));
            assert!(members.contains(&PathBuf::from("special")));
        }

        #[test]
        fn test_resolve_members_deterministic_ordering() {
            use std::fs;

            let temp_dir = tempfile::tempdir().unwrap();
            let workspace_root = temp_dir.path();

            // Create members in non-alphabetical order
            for name in ["zebra", "alpha", "beta"] {
                let dir = workspace_root.join(name);
                fs::create_dir(&dir).unwrap();
                fs::write(dir.join(MANIFEST_FILE), "").unwrap();
            }

            let workspace = Workspace {
                members: Some(vec!["*".to_string()]),
                exclude: None,
            };

            let members = workspace.resolve_members(workspace_root).unwrap();

            // Should be sorted alphabetically
            assert_eq!(members.len(), 3);
            assert_eq!(members[0], PathBuf::from("alpha"));
            assert_eq!(members[1], PathBuf::from("beta"));
            assert_eq!(members[2], PathBuf::from("zebra"));
        }
    }
}
