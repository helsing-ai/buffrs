use crate::ManagedFile;
use crate::errors::{
    DeserializationError, FileExistsError, InvalidManifestError, SerializationError, WriteError,
};
use crate::manifest::{
    Dependency, DependencyMap, Edition, MANIFEST_FILE, ManifestType, PackageManifest, RawManifest,
};
use crate::package::PackageName;
use crate::workspace::Workspace;
use miette::{Context, IntoDiagnostic, bail, miette};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use tokio::fs;

const NO_WORKSPACE: Option<Workspace> = None;
const NO_DEPENDENCIES: Option<DependencyMap> = None;
const NO_PACKAGE: Option<PackageManifest> = None;

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
            "manifest cannot have both dependencies and workspace sections"
        ))
        .wrap_err(InvalidManifestError(ManagedFile::Manifest)),
        (&Some(_), None) => Ok(ManifestType::Package),
        (None, &Some(_)) => Ok(ManifestType::Workspace),
    }
}

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuffrsManifest {
    Package(PackagesManifest),
    Workspace(WorkspaceManifest),
}

impl BuffrsManifest {
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

#[derive(Default, Clone)]
pub struct NoWorkspace;

/// Builder for constructing a WorkspaceManifest
pub struct WorkspaceManifestBuilder<W> {
    workspace: W,
}

impl WorkspaceManifestBuilder<NoWorkspace> {
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

impl GenericManifest for PackagesManifest {}
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
            .map_err(|_| DeserializationError(ManagedFile::Manifest))
            .map(PackagesManifest::try_from)?
    }
}

impl FromStr for WorkspaceManifest {
    type Err = miette::Report;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        input
            .parse::<RawManifest>()
            .map_err(|_| DeserializationError(ManagedFile::Manifest))
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

#[cfg(test)]
mod tests {
    mod raw_manifest_tests {
        use crate::manifest::RawManifest;
        use crate::manifest_v2::BuffrsManifest;
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
        use crate::manifest_v2::{BuffrsManifest, PackagesManifest};
        use crate::package::PackageName;
        use crate::registry::RegistryUri;
        use std::path::PathBuf;
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
            assert!(
                report
                    .to_string()
                    .contains("manifest Proto.toml is invalid")
            )
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

            let manifest_path = PathBuf::from_str(".").unwrap();
            let original_manifest = BuffrsManifest::require_package_manifest(&manifest_path)
                .await
                .expect("should be valid manifest");

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
            edition = "0.12"

            [workspace]
            members = ["pkg1", "pkg2"]
            "#;

            let manifest = BuffrsManifest::from_str(manifest_str).expect("should parse");

            let serialized: String = manifest.try_into().expect("should serialize");
            assert!(serialized.contains("edition"));
            assert!(serialized.contains("[workspace]"));
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
