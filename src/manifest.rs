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
use miette::{Context, IntoDiagnostic, bail, miette};
use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};
use tokio::fs;

use crate::{
    ManagedFile,
    errors::{
        DeserializationError, FileExistsError, InvalidManifestError, SerializationError, WriteError,
    },
    package::{PackageName, PackageType},
    registry::RegistryUri,
    workspace::Workspace,
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

/// Determine the ManifestType based on dependencies, workspace, and package
fn try_manifest_type(
    dependencies: &Option<Vec<Dependency>>,
    workspace: &Option<Workspace>,
    package: &Option<PackageManifest>,
) -> miette::Result<ManifestType> {
    match (&dependencies, &workspace) {
        (&Some(_), &Some(_)) => Err(miette!(
            "manifest cannot have both dependencies and workspace sections"
        ))
        .wrap_err(InvalidManifestError(ManagedFile::Manifest)),
        (None, None) => {
            // Allow package with no dependencies only if package section exists
            if package.is_some() {
                Ok(ManifestType::Package)
            } else {
                Err(miette!(
                    "manifest should have either a package or a workspace section"
                ))
                .wrap_err(InvalidManifestError(ManagedFile::Manifest))
            }
        }
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
    pub fn with_dependencies(&self, dependencies: Vec<Dependency>) -> Self {
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
        RawManifest::Canary {
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
        // Always write as Canary - Unknown manifests get upgraded when written
        RawManifest::Canary {
            package: package_manifest.package,
            dependencies,
            workspace: NO_WORKSPACE,
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
        let package = raw.package().cloned();
        let manifest_type = try_manifest_type(&dependencies, &workspace, &package)?;

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
    use super::*;
    use crate::{package::PackageType, workspace::Workspace};
    use semver::{Version, VersionReq};
    use std::str::FromStr;

    // ===== Edition Tests =====
    mod edition_tests {
        use super::*;

        #[test]
        fn edition_from_str_current_version() {
            let edition = Edition::from(CANARY_EDITION);
            assert_eq!(edition, Edition::Canary);
        }

        #[test]
        fn edition_from_str_legacy_versions() {
            assert_eq!(Edition::from("0.11"), Edition::Canary11);
            assert_eq!(Edition::from("0.10"), Edition::Canary10);
            assert_eq!(Edition::from("0.9"), Edition::Canary09);
            assert_eq!(Edition::from("0.8"), Edition::Canary08);
            assert_eq!(Edition::from("0.7"), Edition::Canary07);
        }

        #[test]
        fn edition_from_str_unknown() {
            assert_eq!(Edition::from("99.99"), Edition::Unknown);
            assert_eq!(Edition::from("invalid"), Edition::Unknown);
            assert_eq!(Edition::from(""), Edition::Unknown);
        }

        #[test]
        fn edition_to_str() {
            assert_eq!(<&str>::from(Edition::Canary), CANARY_EDITION);
            assert_eq!(<&str>::from(Edition::Canary11), "0.11");
            assert_eq!(<&str>::from(Edition::Canary10), "0.10");
            assert_eq!(<&str>::from(Edition::Canary09), "0.9");
            assert_eq!(<&str>::from(Edition::Canary08), "0.8");
            assert_eq!(<&str>::from(Edition::Canary07), "0.7");
            assert_eq!(<&str>::from(Edition::Unknown), "unknown");
        }

        #[test]
        fn edition_latest() {
            assert_eq!(Edition::latest(), Edition::Canary);
        }
    }

    // ===== try_manifest_type Tests =====
    mod manifest_type_tests {
        use super::*;

        #[test]
        fn manifest_type_package() {
            let deps = Some(vec![]);
            let workspace = None;
            let package = None;
            let result = try_manifest_type(&deps, &workspace, &package);
            assert!(result.is_ok());
            assert_eq!(result.unwrap(), ManifestType::Package);
        }

        #[test]
        fn manifest_type_workspace() {
            let deps = None;
            let workspace = Some(Workspace {
                members: vec!["pkg1".to_string()],
                exclude: None,
            });
            let package = None;
            let result = try_manifest_type(&deps, &workspace, &package);
            assert!(result.is_ok());
            assert_eq!(result.unwrap(), ManifestType::Workspace);
        }

        #[test]
        fn manifest_type_both_dependencies_and_workspace_errors() {
            let deps = Some(vec![]);
            let workspace = Some(Workspace {
                members: vec!["pkg1".to_string()],
                exclude: None,
            });
            let package = None;
            let result = try_manifest_type(&deps, &workspace, &package);
            assert!(result.is_err());
            let err_msg = result.unwrap_err().to_string();
            assert!(err_msg.contains("manifest") && err_msg.contains("invalid"));
        }

        #[test]
        fn manifest_type_package_with_no_dependencies() {
            let deps = None;
            let workspace = None;
            let package = Some(PackageManifest {
                kind: PackageType::Lib,
                name: PackageName::new("test").unwrap(),
                version: semver::Version::new(1, 0, 0),
                description: None,
            });
            let result = try_manifest_type(&deps, &workspace, &package);
            assert!(result.is_ok());
            assert_eq!(result.unwrap(), ManifestType::Package);
        }

        #[test]
        fn manifest_type_neither_dependencies_nor_workspace_nor_package_errors() {
            let deps = None;
            let workspace = None;
            let package = None;
            let result = try_manifest_type(&deps, &workspace, &package);
            assert!(result.is_err());
        }
    }

    // ===== RawManifest Tests =====
    mod raw_manifest_tests {
        use super::*;

        #[test]
        fn raw_manifest_accessors_canary() {
            let pkg = PackageManifest {
                kind: PackageType::Lib,
                name: PackageName::from_str("test").unwrap(),
                version: Version::new(1, 0, 0),
                description: None,
            };

            let raw = RawManifest::Canary {
                package: Some(pkg.clone()),
                dependencies: Some(HashMap::new()),
                workspace: None,
            };

            assert_eq!(raw.package(), Some(&pkg));
            assert_eq!(raw.dependencies(), Some(&HashMap::new()));
            assert_eq!(raw.workspace(), None);
            assert_eq!(raw.edition(), Edition::Canary);
        }

        #[test]
        fn raw_manifest_accessors_unknown() {
            let pkg = PackageManifest {
                kind: PackageType::Api,
                name: PackageName::from_str("test").unwrap(),
                version: Version::new(1, 0, 0),
                description: None,
            };

            let raw = RawManifest::Unknown {
                package: Some(pkg.clone()),
                dependencies: None,
                workspace: None,
            };

            assert_eq!(raw.package(), Some(&pkg));
            assert_eq!(raw.dependencies(), None);
            assert_eq!(raw.edition(), Edition::Unknown);
        }

        #[test]
        fn raw_manifest_dependencies_as_vec_empty() {
            let raw = RawManifest::Canary {
                package: None,
                dependencies: Some(HashMap::new()),
                workspace: None,
            };

            assert_eq!(raw.dependencies_as_vec(), Some(vec![]));
        }

        #[test]
        fn raw_manifest_dependencies_as_vec_with_deps() {
            let mut deps = HashMap::new();
            deps.insert(
                PackageName::from_str("test-dep").unwrap(),
                DependencyManifest::Remote(RemoteDependencyManifest {
                    version: VersionReq::from_str("1.0.0").unwrap(),
                    repository: "repo".to_string(),
                    registry: RegistryUri::from_str("https://registry.example.com").unwrap(),
                }),
            );

            let raw = RawManifest::Canary {
                package: None,
                dependencies: Some(deps),
                workspace: None,
            };

            let vec_deps = raw.dependencies_as_vec().unwrap();
            assert_eq!(vec_deps.len(), 1);
            assert_eq!(
                vec_deps[0].package,
                PackageName::from_str("test-dep").unwrap()
            );
        }

        #[test]
        fn raw_manifest_from_str_valid() {
            let toml = r#"
                edition = "0.12"

                [package]
                type = "lib"
                name = "test"
                version = "1.0.0"

                [dependencies]
            "#;

            let raw = RawManifest::from_str(toml).expect("should parse");
            assert!(matches!(raw, RawManifest::Canary { .. }));
        }

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
                .expect("should be convertible to str");
            let raw_manifest_str = toml::to_string(&RawManifest::from(manifest))
                .expect("should be convertible to str");

            assert!(cloned_raw_manifest_str.contains("edition"));
            assert_eq!(cloned_raw_manifest_str, raw_manifest_str);
        }
    }

    // ===== PackagesManifest Tests =====
    mod packages_manifest_tests {
        use super::*;

        #[test]
        fn packages_manifest_builder_defaults() {
            let manifest = PackagesManifest::builder().dependencies(vec![]).build();

            assert_eq!(manifest.edition, Edition::latest());
            assert_eq!(manifest.package, None);
            assert_eq!(manifest.dependencies, Some(vec![]));
        }

        #[test]
        fn packages_manifest_builder_full() {
            let pkg = PackageManifest {
                kind: PackageType::Lib,
                name: PackageName::from_str("test-pkg").unwrap(),
                version: Version::new(1, 2, 3),
                description: Some("A test package".to_string()),
            };

            let deps = vec![Dependency::new(
                RegistryUri::from_str("https://registry.example.com").unwrap(),
                "repo".to_string(),
                PackageName::from_str("dep").unwrap(),
                VersionReq::from_str("1.0.0").unwrap(),
            )];

            let manifest = PackagesManifest::builder()
                .edition(Edition::Canary11)
                .package(pkg.clone())
                .dependencies(deps.clone())
                .build();

            assert_eq!(manifest.edition, Edition::Canary11);
            assert_eq!(manifest.package, Some(pkg));
            assert_eq!(manifest.dependencies, Some(deps));
        }

        #[test]
        fn get_dependency_package_names_empty() {
            let manifest = PackagesManifest::builder().dependencies(vec![]).build();
            assert_eq!(manifest.get_dependency_package_names(), vec![]);
        }

        #[test]
        fn get_dependency_package_names_multiple() {
            let deps = vec![
                Dependency::new(
                    RegistryUri::from_str("https://registry.example.com").unwrap(),
                    "repo".to_string(),
                    PackageName::from_str("dep1").unwrap(),
                    VersionReq::from_str("1.0.0").unwrap(),
                ),
                Dependency::new(
                    RegistryUri::from_str("https://registry.example.com").unwrap(),
                    "repo".to_string(),
                    PackageName::from_str("dep2").unwrap(),
                    VersionReq::from_str("2.0.0").unwrap(),
                ),
            ];

            let manifest = PackagesManifest::builder().dependencies(deps).build();
            let names = manifest.get_dependency_package_names();

            assert_eq!(names.len(), 2);
            assert!(names.contains(&PackageName::from_str("dep1").unwrap()));
            assert!(names.contains(&PackageName::from_str("dep2").unwrap()));
        }

        #[test]
        fn get_dependency_package_names_none() {
            let manifest = PackagesManifest {
                edition: Edition::Canary,
                package: None,
                dependencies: None,
            };
            assert_eq!(manifest.get_dependency_package_names(), vec![]);
        }

        #[test]
        fn clone_with_different_dependencies() {
            let original = PackagesManifest::builder()
                .package(PackageManifest {
                    kind: PackageType::Lib,
                    name: PackageName::from_str("test").unwrap(),
                    version: Version::new(1, 0, 0),
                    description: None,
                })
                .dependencies(vec![])
                .build();

            let new_deps = vec![Dependency::new(
                RegistryUri::from_str("https://registry.example.com").unwrap(),
                "repo".to_string(),
                PackageName::from_str("new-dep").unwrap(),
                VersionReq::from_str("1.0.0").unwrap(),
            )];

            let cloned = original.with_dependencies(new_deps.clone());

            assert_eq!(cloned.dependencies, Some(new_deps));
            assert_eq!(cloned.edition, original.edition);
            assert_eq!(cloned.package, original.package);
        }

        #[test]
        fn get_local_dependencies() {
            let deps = vec![
                Dependency {
                    package: PackageName::from_str("remote").unwrap(),
                    manifest: DependencyManifest::Remote(RemoteDependencyManifest {
                        version: VersionReq::from_str("1.0.0").unwrap(),
                        repository: "repo".to_string(),
                        registry: RegistryUri::from_str("https://registry.example.com").unwrap(),
                    }),
                },
                Dependency {
                    package: PackageName::from_str("local").unwrap(),
                    manifest: DependencyManifest::Local(LocalDependencyManifest {
                        path: PathBuf::from("../local-pkg"),
                    }),
                },
            ];

            let manifest = PackagesManifest::builder().dependencies(deps).build();
            let local_deps = manifest.get_local_dependencies();

            assert_eq!(local_deps.len(), 1);
            assert_eq!(
                local_deps[0].package,
                PackageName::from_str("local").unwrap()
            );
        }

        #[test]
        fn get_remote_dependencies() {
            let deps = vec![
                Dependency {
                    package: PackageName::from_str("remote").unwrap(),
                    manifest: DependencyManifest::Remote(RemoteDependencyManifest {
                        version: VersionReq::from_str("1.0.0").unwrap(),
                        repository: "repo".to_string(),
                        registry: RegistryUri::from_str("https://registry.example.com").unwrap(),
                    }),
                },
                Dependency {
                    package: PackageName::from_str("local").unwrap(),
                    manifest: DependencyManifest::Local(LocalDependencyManifest {
                        path: PathBuf::from("../local-pkg"),
                    }),
                },
            ];

            let manifest = PackagesManifest::builder().dependencies(deps).build();
            let remote_deps = manifest.get_remote_dependencies();

            assert_eq!(remote_deps.len(), 1);
            assert_eq!(
                remote_deps[0].package,
                PackageName::from_str("remote").unwrap()
            );
        }

        #[test]
        fn packages_manifest_from_str_valid() {
            let toml = r#"
                edition = "0.12"

                [package]
                type = "lib"
                name = "test"
                version = "1.0.0"

                [dependencies]
            "#;

            let manifest = PackagesManifest::from_str(toml).expect("should parse");
            assert_eq!(manifest.edition, Edition::Canary);
            assert!(manifest.package.is_some());
        }

        #[test]
        fn packages_manifest_from_str_with_dependencies() {
            let toml = r#"
                edition = "0.12"

                [package]
                type = "lib"
                name = "test"
                version = "1.0.0"

                [dependencies.example]
                version = "1.0.0"
                registry = "https://registry.example.com"
                repository = "my-repo"
            "#;

            let manifest = PackagesManifest::from_str(toml).expect("should parse");
            assert_eq!(manifest.dependencies.as_ref().unwrap().len(), 1);
        }

        #[test]
        fn packages_manifest_to_raw_manifest() {
            let manifest = PackagesManifest::builder()
                .package(PackageManifest {
                    kind: PackageType::Lib,
                    name: PackageName::from_str("test").unwrap(),
                    version: Version::new(1, 0, 0),
                    description: None,
                })
                .dependencies(vec![])
                .build();

            let raw: RawManifest = manifest.into();
            assert!(matches!(raw, RawManifest::Canary { .. }));
            assert!(raw.package().is_some());
            assert_eq!(raw.workspace(), None);
        }

        #[test]
        fn packages_manifest_roundtrip() {
            let toml = r#"
                edition = "0.12"

                [package]
                type = "lib"
                name = "test"
                version = "1.0.0"

                [dependencies]
            "#;

            let manifest = PackagesManifest::from_str(toml).expect("should parse");
            let serialized: String = manifest.try_into().expect("should serialize");

            assert!(serialized.contains("edition"));
            assert!(serialized.contains("[package]"));
            assert!(serialized.contains("test"));
        }
    }

    // ===== WorkspaceManifest Tests =====
    mod workspace_manifest_tests {
        use super::*;

        #[test]
        fn workspace_manifest_builder() {
            let workspace = Workspace {
                members: vec!["pkg1".to_string(), "pkg2".to_string()],
                exclude: Some(vec!["internal".to_string()]),
            };

            let manifest = WorkspaceManifest::builder()
                .workspace(workspace.clone())
                .build();

            assert_eq!(manifest.workspace, workspace);
        }

        #[test]
        fn workspace_manifest_from_str() {
            let toml = r#"
                [workspace]
                members = ["pkg1", "pkg2"]
            "#;

            let manifest = WorkspaceManifest::from_str(toml).expect("should parse");
            assert_eq!(manifest.workspace.members, vec!["pkg1", "pkg2"]);
        }

        #[test]
        fn workspace_manifest_from_str_with_exclude() {
            let toml = r#"
                [workspace]
                members = ["packages/*"]
                exclude = ["packages/internal"]
            "#;

            let manifest = WorkspaceManifest::from_str(toml).expect("should parse");
            assert_eq!(manifest.workspace.members, vec!["packages/*"]);
            assert_eq!(
                manifest.workspace.exclude,
                Some(vec!["packages/internal".to_string()])
            );
        }

        #[test]
        fn workspace_manifest_to_raw_manifest() {
            let workspace = Workspace {
                members: vec!["pkg1".to_string()],
                exclude: None,
            };

            let manifest = WorkspaceManifest::builder().workspace(workspace).build();
            let raw: RawManifest = manifest.into();

            assert!(matches!(raw, RawManifest::Canary { .. }));
            assert!(raw.workspace().is_some());
            assert_eq!(raw.package(), None);
            assert_eq!(raw.dependencies(), None);
        }

        #[test]
        fn workspace_manifest_roundtrip() {
            let toml = r#"
                [workspace]
                members = ["pkg1", "pkg2"]
            "#;

            let manifest = WorkspaceManifest::from_str(toml).expect("should parse");
            let serialized: String = manifest.try_into().expect("should serialize");

            assert!(serialized.contains("[workspace]"));
            assert!(serialized.contains("pkg1"));
            assert!(serialized.contains("pkg2"));
        }

        #[test]
        fn workspace_manifest_try_from_raw_missing_workspace_errors() {
            let raw = RawManifest::Canary {
                package: None,
                dependencies: Some(HashMap::new()),
                workspace: None,
            };

            let result = WorkspaceManifest::try_from(raw);
            assert!(result.is_err());
            assert!(
                result
                    .unwrap_err()
                    .to_string()
                    .contains("no workspace manifest")
            );
        }
    }

    // ===== BuffrsManifest Tests =====
    mod buffrs_manifest_tests {
        use super::*;

        #[test]
        fn buffrs_manifest_package_from_str() {
            let toml = r#"
                edition = "0.12"

                [package]
                type = "lib"
                name = "test"
                version = "1.0.0"

                [dependencies]
            "#;

            let manifest = BuffrsManifest::from_str(toml).expect("should parse");
            assert!(matches!(manifest, BuffrsManifest::Package(_)));
        }

        #[test]
        fn buffrs_manifest_workspace_from_str() {
            let toml = r#"
                [workspace]
                members = ["pkg1"]
            "#;

            let manifest = BuffrsManifest::from_str(toml).expect("should parse");
            assert!(matches!(manifest, BuffrsManifest::Workspace(_)));
        }

        #[test]
        fn buffrs_manifest_invalid_mixed() {
            let toml = r#"
                [workspace]
                members = ["pkg1"]

                [dependencies]
            "#;

            let result = BuffrsManifest::from_str(toml);
            assert!(result.is_err());
        }

        #[test]
        fn buffrs_manifest_invalid_empty() {
            let result = BuffrsManifest::from_str("");
            assert!(result.is_err());
        }

        #[tokio::test]
        async fn buffrs_manifest_to_package_manifest_success() {
            let toml = r#"
                edition = "0.12"

                [package]
                type = "lib"
                name = "test"
                version = "1.0.0"

                [dependencies]
            "#;

            let manifest = BuffrsManifest::from_str(toml).expect("should parse");
            let result = manifest.to_package_manifest().await;
            assert!(result.is_ok());
        }

        #[tokio::test]
        async fn buffrs_manifest_to_package_manifest_fails_for_workspace() {
            let toml = r#"
                [workspace]
                members = ["pkg1"]
            "#;

            let manifest = BuffrsManifest::from_str(toml).expect("should parse");
            let result = manifest.to_package_manifest().await;
            assert!(result.is_err());
            assert!(
                result
                    .unwrap_err()
                    .to_string()
                    .contains("packages manifest is required")
            );
        }

        #[test]
        fn buffrs_manifest_roundtrip_package() {
            let toml = r#"
                edition = "0.12"

                [package]
                type = "lib"
                name = "test"
                version = "1.0.0"

                [dependencies]
            "#;

            let manifest = BuffrsManifest::from_str(toml).expect("should parse");
            let serialized: String = manifest.try_into().expect("should serialize");

            assert!(serialized.contains("edition"));
            assert!(serialized.contains("[package]"));
        }

        #[test]
        fn buffrs_manifest_roundtrip_workspace() {
            let toml = r#"
                [workspace]
                members = ["pkg1", "pkg2"]
            "#;

            let manifest = BuffrsManifest::from_str(toml).expect("should parse");
            let serialized: String = manifest.try_into().expect("should serialize");

            assert!(serialized.contains("[workspace]"));
        }

        #[test]
        fn unknown_edition_rejected() {
            let toml = r#"
                edition = "99.99"

                [package]
                type = "lib"
                name = "test"
                version = "0.0.1"

                [dependencies]
            "#;

            let result = PackagesManifest::from_str(toml);
            assert!(result.is_err());
        }

        #[test]
        fn manifest_without_edition_becomes_unknown() {
            let toml = r#"
                [package]
                type = "lib"
                name = "test"
                version = "0.0.1"

                [dependencies]
            "#;

            let manifest = PackagesManifest::from_str(toml).expect("should parse");
            assert_eq!(manifest.edition, Edition::Unknown);
        }
    }

    // ===== Dependency Tests =====
    mod dependency_tests {
        use super::*;

        #[test]
        fn dependency_new() {
            let dep = Dependency::new(
                RegistryUri::from_str("https://registry.example.com").unwrap(),
                "my-repo".to_string(),
                PackageName::from_str("test-pkg").unwrap(),
                VersionReq::from_str("1.2.3").unwrap(),
            );

            assert_eq!(dep.package, PackageName::from_str("test-pkg").unwrap());
            match dep.manifest {
                DependencyManifest::Remote(ref remote) => {
                    assert_eq!(remote.repository, "my-repo");
                    assert_eq!(
                        remote.registry,
                        RegistryUri::from_str("https://registry.example.com").unwrap()
                    );
                }
                _ => panic!("Expected remote dependency"),
            }
        }

        #[test]
        fn dependency_with_version() {
            let dep = Dependency::new(
                RegistryUri::from_str("https://registry.example.com").unwrap(),
                "repo".to_string(),
                PackageName::from_str("test").unwrap(),
                VersionReq::from_str("1.0.0").unwrap(),
            );

            let pinned = dep.with_version(&Version::new(2, 3, 4));

            match pinned.manifest {
                DependencyManifest::Remote(ref remote) => {
                    assert_eq!(remote.version.to_string(), "=2.3.4");
                }
                _ => panic!("Expected remote dependency"),
            }
        }

        #[test]
        fn dependency_with_version_prerelease() {
            let dep = Dependency::new(
                RegistryUri::from_str("https://registry.example.com").unwrap(),
                "repo".to_string(),
                PackageName::from_str("test").unwrap(),
                VersionReq::from_str("1.0.0").unwrap(),
            );

            let mut version = Version::new(1, 0, 0);
            version.pre = semver::Prerelease::new("alpha.1").unwrap();

            let pinned = dep.with_version(&version);

            match pinned.manifest {
                DependencyManifest::Remote(ref remote) => {
                    assert_eq!(remote.version.to_string(), "=1.0.0-alpha.1");
                }
                _ => panic!("Expected remote dependency"),
            }
        }

        #[test]
        fn dependency_with_version_local_unchanged() {
            let dep = Dependency {
                package: PackageName::from_str("test").unwrap(),
                manifest: DependencyManifest::Local(LocalDependencyManifest {
                    path: PathBuf::from("../test"),
                }),
            };

            let cloned = dep.with_version(&Version::new(1, 0, 0));
            assert_eq!(dep, cloned);
        }

        #[test]
        fn dependency_display_remote() {
            let dep = Dependency::new(
                RegistryUri::from_str("https://registry.example.com").unwrap(),
                "my-repo".to_string(),
                PackageName::from_str("test-pkg").unwrap(),
                VersionReq::from_str("1.2.3").unwrap(),
            );

            let display = format!("{}", dep);
            assert!(display.contains("my-repo"));
            assert!(display.contains("test-pkg"));
            assert!(display.contains("1.2.3"));
        }

        #[test]
        fn dependency_display_local() {
            let dep = Dependency {
                package: PackageName::from_str("local-pkg").unwrap(),
                manifest: DependencyManifest::Local(LocalDependencyManifest {
                    path: PathBuf::from("../local-pkg"),
                }),
            };

            let display = format!("{}", dep);
            assert!(display.contains("local-pkg"));
            assert!(display.contains("local-pkg")); // path contains name
        }

        #[test]
        fn dependency_manifest_is_local() {
            let local = DependencyManifest::Local(LocalDependencyManifest {
                path: PathBuf::from("../test"),
            });
            assert!(local.is_local());

            let remote = DependencyManifest::Remote(RemoteDependencyManifest {
                version: VersionReq::from_str("1.0.0").unwrap(),
                repository: "repo".to_string(),
                registry: RegistryUri::from_str("https://registry.example.com").unwrap(),
            });
            assert!(!remote.is_local());
        }
    }

    // ===== Serialization/Deserialization Edge Cases =====
    mod serialization_tests {
        use super::*;

        #[test]
        fn serialize_package_manifest_with_description() {
            let manifest = PackagesManifest::builder()
                .package(PackageManifest {
                    kind: PackageType::Api,
                    name: PackageName::from_str("test").unwrap(),
                    version: Version::new(1, 0, 0),
                    description: Some("Test description".to_string()),
                })
                .dependencies(vec![])
                .build();

            let serialized: String = manifest.try_into().expect("should serialize");
            assert!(serialized.contains("Test description"));
        }

        #[test]
        fn deserialize_manifest_with_local_dependency() {
            let toml = r#"
                edition = "0.12"

                [package]
                type = "lib"
                name = "test"
                version = "1.0.0"

                [dependencies.local-dep]
                path = "../local-dep"
            "#;

            let manifest = PackagesManifest::from_str(toml).expect("should parse");
            let deps = manifest.dependencies.unwrap();
            assert_eq!(deps.len(), 1);
            assert!(deps[0].manifest.is_local());
        }

        #[test]
        fn deserialize_manifest_multiple_dependencies() {
            let toml = r#"
                edition = "0.12"

                [package]
                type = "lib"
                name = "test"
                version = "1.0.0"

                [dependencies.dep1]
                version = "1.0.0"
                registry = "https://registry.example.com"
                repository = "repo1"

                [dependencies.dep2]
                path = "../local"

                [dependencies.dep3]
                version = "2.0.0"
                registry = "https://other-registry.example.com"
                repository = "repo2"
            "#;

            let manifest = PackagesManifest::from_str(toml).expect("should parse");
            let deps = manifest.dependencies.unwrap();
            assert_eq!(deps.len(), 3);
        }

        #[test]
        fn upgrade_unknown_to_canary_on_write() {
            let toml = r#"
                [package]
                type = "lib"
                name = "test"
                version = "1.0.0"

                [dependencies]
            "#;

            let manifest = PackagesManifest::from_str(toml).expect("should parse");
            assert_eq!(manifest.edition, Edition::Unknown);

            let serialized: String = manifest.try_into().expect("should serialize");
            // When written, Unknown gets upgraded to Canary
            assert!(serialized.contains("edition"));
        }

        #[test]
        fn deserialize_invalid_toml() {
            let invalid_toml = "this is not valid toml {]";
            let result = PackagesManifest::from_str(invalid_toml);
            assert!(result.is_err());
        }

        #[test]
        fn deserialize_missing_required_package_fields() {
            let toml = r#"
                edition = "0.12"

                [package]
                name = "test"

                [dependencies]
            "#;

            let result = PackagesManifest::from_str(toml);
            assert!(result.is_err());
        }
    }

    // ===== Integration Tests =====
    mod integration_tests {
        use super::*;

        #[test]
        fn complex_package_manifest_full_cycle() {
            // Create a complex manifest with multiple dependencies
            let pkg = PackageManifest {
                kind: PackageType::Lib,
                name: PackageName::from_str("complex-package").unwrap(),
                version: Version::new(2, 1, 0),
                description: Some("A complex test package".to_string()),
            };

            let deps = vec![
                Dependency::new(
                    RegistryUri::from_str("https://registry1.example.com").unwrap(),
                    "repo1".to_string(),
                    PackageName::from_str("remote-dep-1").unwrap(),
                    VersionReq::from_str("1.0.0").unwrap(),
                ),
                Dependency {
                    package: PackageName::from_str("local-dep").unwrap(),
                    manifest: DependencyManifest::Local(LocalDependencyManifest {
                        path: PathBuf::from("../local"),
                    }),
                },
                Dependency::new(
                    RegistryUri::from_str("https://registry2.example.com").unwrap(),
                    "repo2".to_string(),
                    PackageName::from_str("remote-dep-2").unwrap(),
                    VersionReq::from_str("2.3.4").unwrap(),
                ),
            ];

            let manifest = PackagesManifest::builder()
                .edition(Edition::Canary)
                .package(pkg.clone())
                .dependencies(deps.clone())
                .build();

            // Convert to string
            let serialized: String = manifest.clone().try_into().expect("should serialize");

            // Parse back
            let parsed = PackagesManifest::from_str(&serialized).expect("should parse");

            // Verify roundtrip
            assert_eq!(parsed.edition, manifest.edition);
            assert_eq!(parsed.package, manifest.package);
            assert_eq!(parsed.dependencies.as_ref().unwrap().len(), 3);

            // Verify dependencies are correct types
            let local_deps = parsed.get_local_dependencies();
            let remote_deps = parsed.get_remote_dependencies();
            assert_eq!(local_deps.len(), 1);
            assert_eq!(remote_deps.len(), 2);
        }

        #[test]
        fn workspace_manifest_full_cycle() {
            let workspace = Workspace {
                members: vec!["packages/*".to_string(), "special-package".to_string()],
                exclude: Some(vec!["packages/internal*".to_string()]),
            };

            let manifest = WorkspaceManifest::builder().workspace(workspace).build();

            // Convert to string
            let serialized: String = manifest.clone().try_into().expect("should serialize");

            // Parse back
            let parsed = WorkspaceManifest::from_str(&serialized).expect("should parse");

            // Verify roundtrip
            assert_eq!(parsed.workspace.members, manifest.workspace.members);
            assert_eq!(parsed.workspace.exclude, manifest.workspace.exclude);
        }
    }
}
