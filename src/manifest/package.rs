// Copyright 2026 Helsing GmbH
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
use miette::{Context, IntoDiagnostic};
use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};

use super::Edition;
use super::MANIFEST_FILE;
use super::raw::RawManifest;
use crate::{
    ManagedFile,
    errors::DeserializationError,
    io::File,
    package::{PackageName, PackageType},
    registry::RegistryUri,
};

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

    /// Get package names of dependencies
    pub fn get_dependency_package_names(&self) -> Vec<PackageName> {
        self.dependencies
            .iter()
            .flatten()
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
        self.dependencies
            .iter()
            .flatten()
            .filter(|d| predicate(d))
            .cloned()
            .collect()
    }

    /// Applies a version override to the package manifest if both version and package are present
    ///
    /// This is a convenience method to hide the `if let Some` logic used when overriding
    /// package versions during publish or package operations.
    pub fn with_version(mut self, version: Option<Version>) -> Self {
        if let Some(version) = version
            && let Some(ref mut package) = self.package
        {
            tracing::info!(
                "modified version in published manifest for {} from {} to {}",
                package.name,
                package.version,
                version
            );
            package.version = version;
        }
        self
    }
}

/// A [`PackagesManifest`] guaranteed to contain a `[package]` declaration.
///
/// Created via [`PublishableManifest::try_new`], which returns `None` when
/// the manifest has no package section (e.g. dependency-only workspace members).
#[derive(Clone, Debug)]
pub struct PublishableManifest(PackagesManifest);

impl PublishableManifest {
    /// Creates a new `PublishableManifest` if the manifest contains a package declaration.
    ///
    /// Returns `None` when `manifest.package` is `None`.
    pub fn try_new(manifest: PackagesManifest) -> Option<Self> {
        manifest.package.as_ref()?;
        Some(Self(manifest))
    }

    /// Returns a reference to the package metadata.
    ///
    /// This never fails because `PublishableManifest` guarantees a package is present.
    pub fn package(&self) -> &PackageManifest {
        self.0
            .package
            .as_ref()
            .expect("PublishableManifest guarantees package is Some")
    }

    /// Returns a reference to the inner `PackagesManifest`.
    pub fn inner(&self) -> &PackagesManifest {
        &self.0
    }

    /// Unwraps the inner `PackagesManifest`.
    pub fn into_inner(self) -> PackagesManifest {
        self.0
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

#[async_trait]
impl File for PackagesManifest {
    const DEFAULT_PATH: &str = MANIFEST_FILE;

    async fn load_from<P>(path: P) -> miette::Result<Self>
    where
        P: AsRef<Path> + Send + Sync,
    {
        RawManifest::load_from(path).await?.try_into()
    }

    async fn save_to<P>(&self, path: P) -> miette::Result<()>
    where
        P: AsRef<Path> + Send + Sync,
    {
        RawManifest::from(self.clone()).save_to(path).await
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

impl TryInto<String> for PackagesManifest {
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
    /// List of paths that should be **included** in the package.
    ///
    /// Gitignore syntax is supported. Starts from an empty set: only
    /// files matching one of the globs are included (any file type,
    /// not limited to `.proto`).
    ///
    /// If neither `include` nor `exclude` is set, the default is
    /// every `.proto` file under the package root.
    ///
    /// Mutually exclusive with `exclude`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include: Option<Vec<String>>,
    /// List of paths that should be **excluded** from the package.
    ///
    /// Gitignore syntax is supported. Starts from the set of all files
    /// under the package root (any file type, not limited to `.proto`)
    /// and removes files matching any of the globs.
    ///
    /// Mutually exclusive with `include`.
    #[serde(default)]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub exclude: Vec<String>,
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

/// Map representation of the dependency list
pub type DependencyMap = HashMap<PackageName, DependencyManifest>;

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
            include: Default::default(),
            exclude: Default::default(),
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
                include: Default::default(),
                exclude: Default::default(),
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
                edition = "0.13"

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
                include: Default::default(),
                exclude: Default::default(),
            })
            .dependencies(vec![])
            .build();

        let raw: RawManifest = manifest.into();
        assert!(matches!(raw, RawManifest::Canary { .. }));
        assert!(raw.package().is_some());
        assert_eq!(raw.workspace(), None);
    }

    #[test]
    fn publishable_manifest_try_new_returns_none_without_package() {
        let manifest = PackagesManifest::builder()
            .dependencies(Default::default())
            .build();

        assert!(PublishableManifest::try_new(manifest).is_none());
    }

    #[test]
    fn publishable_manifest_try_new_returns_some_with_package() {
        let manifest = PackagesManifest::builder()
            .package(PackageManifest {
                kind: PackageType::Lib,
                name: PackageName::from_str("test-pkg").unwrap(),
                version: Version::new(1, 0, 0),
                description: None,
                include: Default::default(),
                exclude: Default::default(),
            })
            .dependencies(Default::default())
            .build();

        let publishable = PublishableManifest::try_new(manifest).expect("should be Some");
        assert_eq!(
            publishable.package().name,
            PackageName::from_str("test-pkg").unwrap()
        );
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
