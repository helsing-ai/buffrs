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
    path::{Path, PathBuf},
    str::FromStr,
};

use async_trait::async_trait;
use miette::{bail, miette, Context, IntoDiagnostic};
use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};
use tokio::fs;

use crate::{
    errors::{
        DeserializationError, FileExistsError, InvalidManifestError, SerializationError,
        WriteError,
    },
    package::{PackageName, PackageType},
    registry::RegistryUri,
    workspace::Workspace,
    ManagedFile,
};

/// The name of the manifest file
pub const MANIFEST_FILE: &str = "Proto.toml";

/// The canary edition supported by this version of buffrs
pub const CANARY_EDITION: &str = concat!("0.", env!("CARGO_PKG_VERSION_MINOR"));

const NO_WORKSPACE: Option<Workspace> = None;
const NO_DEPENDENCIES: Option<DependencyMap> = None;
const NO_PACKAGE: Option<PackageManifest> = None;

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

/// Determine the ManifestType based on dependencies and workspace
fn try_manifest_type(
    dependencies: &Option<Vec<Dependency>>,
    workspace: &Option<Workspace>,
) -> miette::Result<ManifestType> {
    match (&dependencies, &workspace) {
        (&Some(_), &Some(_)) => Err(miette!(
            "manifest cannot have both dependencies and workspace sections"
        ))
        .wrap_err(InvalidManifestError(ManagedFile::Manifest)),
        (None, None) => Err(miette!(
            "manifest should have either dependencies or a workspace section"
        ))
        .wrap_err(InvalidManifestError(ManagedFile::Manifest)),
        (&Some(_), None) => Ok(ManifestType::Package),
        (None, &Some(_)) => Ok(ManifestType::Workspace),
    }
}

/// Defines common interfaces any Manifest should support
#[async_trait]
pub trait GenericManifest: Sized + Into<RawManifest> + TryInto<String> + FromStr + Clone {
    /// Checks if the manifest file exists in the filesystem
    async fn exists() -> miette::Result<bool> {
        fs::try_exists(MANIFEST_FILE)
            .await
            .into_diagnostic()
            .wrap_err(FileExistsError(MANIFEST_FILE))
    }

    /// Persists the manifest into the current directory
    async fn write(&self) -> miette::Result<()> {
        self.write_at(Path::new(".")).await
    }

    /// Persists the manifest into the provided directory, which must exist
    async fn write_at(&self, dir_path: &Path) -> miette::Result<()> {
        // hint: create a canary manifest from the current one by cloning fields
        let raw: RawManifest = self.clone().into();

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

/// A buffrs manifest enum describing the different types of manifests that buffrs understands
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuffrsManifest {
    /// A package manifest describing a concrete package
    Package(PackagesManifest),
    /// A workspace manifest defining a buffrs workspace
    Workspace(WorkspaceManifest),
}

impl BuffrsManifest {
    /// Returns a human friendly representation the current package. Intended to be used in error messages on the CLI top level
    pub async fn current_dir_display_name() -> Option<String> {
        let manifest = BuffrsManifest::try_read().await.ok()?;

        let cwd = std::env::current_dir().unwrap();

        let path_name = cwd.file_name()?.to_str();

        match manifest {
            BuffrsManifest::Package(p) => Some(p.package?.name.to_string()),
            BuffrsManifest::Workspace(_) => path_name.map(String::from),
        }
    }
    /// Ensures the current directory contains a package manifest, not a workspace
    ///
    /// Returns an error if the manifest is a workspace manifest, otherwise the package manifest
    /// Use this at the beginning of commands that don't support workspaces.
    pub async fn require_package_manifest(path: &PathBuf) -> miette::Result<PackagesManifest> {
        let manifest = BuffrsManifest::try_read_from(path).await?;

        match manifest {
            BuffrsManifest::Package(packages_manifest) => Ok(packages_manifest),
            BuffrsManifest::Workspace(_) => {
                bail!("A packages manifest is required, but a workspace manifest was found")
            }
        }
    }

    /// Returns the packages manifest if correct type, errs otherwise
    pub async fn to_package_manifest(self) -> miette::Result<PackagesManifest> {
        match self {
            BuffrsManifest::Package(packages_manifest) => Ok(packages_manifest),
            BuffrsManifest::Workspace(_) => {
                bail!("A packages manifest is required, but a workspace manifest was found")
            }
        }
    }

    /// Checks if a manifest file exists in the filesystem
    pub async fn exists() -> miette::Result<bool> {
        fs::try_exists(MANIFEST_FILE)
            .await
            .into_diagnostic()
            .wrap_err(FileExistsError(MANIFEST_FILE))
    }

    /// Loads the manifest from the current directory
    pub async fn try_read() -> miette::Result<Self> {
        Self::try_read_from(MANIFEST_FILE)
            .await
            .wrap_err(miette!("`{MANIFEST_FILE}` does not exist"))
    }

    /// Loads the manifest from the given path
    pub async fn try_read_from(path: impl AsRef<Path>) -> miette::Result<Self> {
        let contents = match fs::read_to_string(path.as_ref()).await {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
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
}

/// A manifest for a buffrs package
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackagesManifest {
    /// Edition of this manifest
    pub edition: Edition,
    /// Metadata about the root package
    pub package: Option<PackageManifest>,
    /// List of packages the root package depends on
    pub dependencies: Option<Vec<Dependency>>,
}

impl PackagesManifest {
    /// Create a new builder for PackagesManifest
    pub fn builder() -> PackagesManifestBuilder {
        PackagesManifestBuilder {
            edition: Edition::latest(),
            package: None,
            dependencies: None,
        }
    }
}

/// Builder for constructing a PackagesManifest
pub struct PackagesManifestBuilder {
    edition: Edition,
    package: Option<PackageManifest>,
    dependencies: Option<Vec<Dependency>>,
}

impl PackagesManifestBuilder {
    /// Sets the edition
    pub fn edition(mut self, edition: Edition) -> Self {
        self.edition = edition;
        self
    }

    /// Sets the package metadata
    pub fn package(mut self, package: PackageManifest) -> Self {
        self.package = Some(package);
        self
    }

    /// Sets the dependencies
    pub fn dependencies(mut self, dependencies: Vec<Dependency>) -> Self {
        self.dependencies = Some(dependencies);
        self
    }

    /// Builds the PackagesManifest
    pub fn build(self) -> PackagesManifest {
        PackagesManifest {
            edition: self.edition,
            package: self.package,
            dependencies: self.dependencies,
        }
    }
}

/// A manifest for a buffrs workspace
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceManifest {
    /// Definition of a buffrs workspace
    pub workspace: Workspace,
}

impl WorkspaceManifest {
    /// Create a new builder for WorkspaceManifest
    pub fn builder() -> WorkspaceManifestBuilder<NoWorkspace> {
        WorkspaceManifestBuilder {
            workspace: NoWorkspace,
        }
    }
}

/// NoWorkspace Type used for the workspace typestate builder pattern
#[derive(Default, Clone)]
pub struct NoWorkspace;

/// Builder for constructing a WorkspaceManifest
pub struct WorkspaceManifestBuilder<W> {
    workspace: W,
}

impl WorkspaceManifestBuilder<NoWorkspace> {
    /// Set the workspace and transition to a WorkspaceManifestBuilder<Workspace>
    pub fn workspace(self, workspace: Workspace) -> WorkspaceManifestBuilder<Workspace> {
        WorkspaceManifestBuilder { workspace }
    }
}

impl WorkspaceManifestBuilder<Workspace> {
    /// Builds the WorkspaceManifest
    pub fn build(self) -> WorkspaceManifest {
        WorkspaceManifest {
            workspace: self.workspace,
        }
    }
}

#[async_trait]
impl GenericManifest for PackagesManifest {}
#[async_trait]
impl GenericManifest for WorkspaceManifest {}

impl PackagesManifest {
    /// Get package names of dependencies
    pub fn get_dependency_package_names(&self) -> Vec<PackageName> {
        self.dependencies
            .clone()
            .unwrap_or_default()
            .iter()
            .map(|d| d.package.clone())
            .collect()
    }

    /// Clones the Manifest but replaces the dependencies with a given Vec
    pub fn clone_with_different_dependencies(&self, dependencies: Vec<Dependency>) -> Self {
        Self {
            dependencies: Some(dependencies),
            ..self.clone()
        }
    }

    /// Gets a list of all local dependencies
    pub fn get_local_dependencies(&self) -> Vec<Dependency> {
        self.get_dependencies_of_type(|d| d.manifest.is_local())
    }

    /// Gets a list of all local dependencies
    pub fn get_remote_dependencies(&self) -> Vec<Dependency> {
        self.get_dependencies_of_type(|d| !d.manifest.is_local())
    }

    /// Gets a list of all dependencies
    fn get_dependencies_of_type(&self, predicate: fn(d: &Dependency) -> bool) -> Vec<Dependency> {
        self.clone()
            .dependencies
            .unwrap_or_default()
            .into_iter()
            .filter(predicate)
            .collect()
    }
}

impl From<BuffrsManifest> for RawManifest {
    fn from(manifest: BuffrsManifest) -> Self {
        match manifest {
            BuffrsManifest::Package(package_manifest) => package_manifest.into(),
            BuffrsManifest::Workspace(workspace_manifest) => workspace_manifest.into(),
        }
    }
}

impl FromStr for PackagesManifest {
    type Err = miette::Report;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        input
            .parse::<RawManifest>()
            .into_diagnostic()
            .wrap_err(DeserializationError(ManagedFile::Manifest))
            .map(PackagesManifest::try_from)?
    }
}

impl FromStr for WorkspaceManifest {
    type Err = miette::Report;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        input
            .parse::<RawManifest>()
            .into_diagnostic()
            .wrap_err(DeserializationError(ManagedFile::Manifest))
            .map(WorkspaceManifest::try_from)?
    }
}

impl From<WorkspaceManifest> for RawManifest {
    fn from(workspace_manifest: WorkspaceManifest) -> Self {
        RawManifest::Unknown {
            package: NO_PACKAGE,
            dependencies: NO_DEPENDENCIES,
            workspace: Some(workspace_manifest.workspace),
        }
    }
}

impl TryInto<String> for PackagesManifest {
    type Error = toml::ser::Error;

    fn try_into(self) -> Result<String, Self::Error> {
        toml::to_string_pretty(&RawManifest::from(self))
    }
}

impl TryInto<String> for WorkspaceManifest {
    type Error = toml::ser::Error;

    fn try_into(self) -> Result<String, Self::Error> {
        toml::to_string_pretty(&RawManifest::from(self))
    }
}

impl From<PackagesManifest> for RawManifest {
    fn from(package_manifest: PackagesManifest) -> Self {
        let dependencies = package_manifest.dependencies.map(|deps| {
            deps.into_iter()
                .map(|dep| (dep.package, dep.manifest))
                .collect()
        });
        match package_manifest.edition {
            Edition::Unknown => RawManifest::Unknown {
                package: package_manifest.package,
                dependencies,
                workspace: NO_WORKSPACE,
            },
            _ => RawManifest::Canary {
                package: package_manifest.package,
                dependencies,
                workspace: NO_WORKSPACE,
            },
        }
    }
}

impl TryFrom<RawManifest> for WorkspaceManifest {
    type Error = miette::Report;

    fn try_from(raw: RawManifest) -> Result<Self, Self::Error> {
        if raw.workspace().is_none() {
            bail!("Manifest has no workspace manifest");
        }

        match raw.workspace() {
            None => bail!("Manifest has no workspace manifest"),
            Some(workspace_manifest) => Ok(WorkspaceManifest::builder()
                .workspace(workspace_manifest.clone())
                .build()),
        }
    }
}

impl TryFrom<RawManifest> for PackagesManifest {
    type Error = miette::Report;

    fn try_from(raw: RawManifest) -> Result<Self, Self::Error> {
        Ok(PackagesManifest {
            edition: raw.edition(),
            package: raw.package().cloned(),
            dependencies: raw.dependencies_as_vec(),
        })
    }
}

impl TryFrom<RawManifest> for BuffrsManifest {
    type Error = miette::Report;

    fn try_from(raw: RawManifest) -> Result<BuffrsManifest, Self::Error> {
        let dependencies = raw.dependencies_as_vec();
        let workspace = raw.workspace().cloned();
        let manifest_type = try_manifest_type(&dependencies, &workspace)?;

        let manifest = match manifest_type {
            ManifestType::Package => BuffrsManifest::Package(raw.try_into()?),
            ManifestType::Workspace => BuffrsManifest::Workspace(raw.try_into()?),
        };

        Ok(manifest)
    }
}

impl FromStr for BuffrsManifest {
    type Err = miette::Report;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        input
            .parse::<RawManifest>()
            .into_diagnostic()
            .map(Self::try_from)?
    }
}

impl TryInto<String> for BuffrsManifest {
    type Error = toml::ser::Error;

    fn try_into(self) -> Result<String, Self::Error> {
        match self {
            BuffrsManifest::Package(p) => p.try_into(),
            BuffrsManifest::Workspace(w) => w.try_into(),
        }
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

#[cfg(test)]
mod tests {
    mod raw_manifest_tests {
        use crate::manifest::RawManifest;
        use crate::manifest::BuffrsManifest;
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

            let manifest = BuffrsManifest::from_str(manifest).expect("should be valid manifest");
            let cloned_raw_manifest_str = toml::to_string(&RawManifest::from(manifest.clone()))
                .expect("should be convertable to str");
            let raw_manifest_str = toml::to_string(&RawManifest::from(manifest))
                .expect("should be convertable to str");

            assert!(cloned_raw_manifest_str.contains("edition"));
            assert_eq!(cloned_raw_manifest_str, raw_manifest_str);
        }
    }
    mod manifest_tests {
        use crate::manifest::Edition;
        use crate::manifest::{BuffrsManifest, PackagesManifest};
        use crate::package::PackageName;
        use crate::registry::RegistryUri;
        use std::str::FromStr;

        #[test]
        fn invalid_mixed_manifest() {
            let mixed_dep_and_workspace = r#"
        [workspace]

        [dependencies]
        "#;
            let manifest = BuffrsManifest::from_str(mixed_dep_and_workspace);
            assert!(manifest.is_err());
            let report = manifest.err().unwrap();
            println!("{}", report.to_string());
            assert!(report.to_string().contains("missing field `members`"))
        }

        #[test]
        fn invalid_empty_manifest() {
            let empty_manifest = "";
            let manifest = BuffrsManifest::from_str(empty_manifest);
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

            let manifest = BuffrsManifest::from_str(manifest).expect("should be valid manifest");

            assert!(matches!(manifest, BuffrsManifest::Package(_)));
        }

        #[tokio::test]
        async fn test_clone_with_different_dependencies() {
            use crate::manifest::Dependency;
            use semver::VersionReq;
            use std::str::FromStr;

            // Create original manifest with initial dependencies
            let manifest = r#"
            edition = "0.12"

            [package]
            type = "lib"
            name = "test-package"
            version = "1.0.0"

            [dependencies.test-dependency]
            version = "1.0.0"
            registry = "https://registry.example.com"
            repository = "original-repo"
            "#;

            let original_manifest = BuffrsManifest::from_str(manifest)
                .expect("should be valid manifest")
                .to_package_manifest()
                .await
                .expect("should be package manifest");

            // Create new dependencies
            let new_deps = vec![
                Dependency::new(
                    RegistryUri::from_str("https://new-registry.example.com").unwrap(),
                    "new-repo".to_string(),
                    PackageName::from_str("new-dep-1").unwrap(),
                    VersionReq::from_str("2.0.0").unwrap(),
                ),
                Dependency::new(
                    RegistryUri::from_str("https://another-registry.example.com").unwrap(),
                    "another-repo".to_string(),
                    PackageName::from_str("new-dep-2").unwrap(),
                    VersionReq::from_str("3.0.0").unwrap(),
                ),
            ];

            // Clone with different dependencies
            let cloned_manifest =
                original_manifest.clone_with_different_dependencies(new_deps.clone());

            // Verify the dependencies were replaced
            assert_eq!(cloned_manifest.dependencies, Some(new_deps));

            // Verify other fields remain unchanged
            assert_eq!(cloned_manifest.edition, original_manifest.edition);
            assert_eq!(cloned_manifest.package, original_manifest.package);
        }

        #[test]
        fn workspace_manifest_roundtrip() {
            let manifest_str = r#"
            [workspace]
            members = ["pkg1", "pkg2"]
            "#;

            let manifest = BuffrsManifest::from_str(manifest_str).expect("should parse");

            let serialized: String = manifest.try_into().expect("should serialize");
            assert!(serialized.contains("[workspace]"));
        }

        #[test]
        fn unknown_edition_parsed_rejected() {
            let manifest_str = r#"
            edition = "99.99"

            [package]
            type = "lib"
            name = "test"
            version = "0.0.1"

            [dependencies]
            "#;

            let result = PackagesManifest::from_str(manifest_str);

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

            let manifest = PackagesManifest::from_str(manifest_str).expect("should parse");
            assert_eq!(manifest.edition, Edition::Unknown);
        }
    }
}
