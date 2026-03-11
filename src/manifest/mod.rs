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
    path::{Path, PathBuf},
    str::FromStr,
};

use async_trait::async_trait;
use miette::IntoDiagnostic;
use serde::{Deserialize, Serialize};

use crate::io::File;

/// Package manifest types and dependency definitions
pub mod package;
/// Raw manifest serialization format
pub mod raw;
/// Workspace manifest types
pub mod workspace;

pub use package::*;
pub use raw::*;
pub use workspace::*;

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

/// A buffrs manifest enum describing the different types of manifests that buffrs understands
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Manifest {
    /// A package manifest describing a concrete package
    Package(PackagesManifest),
    /// A workspace manifest defining a buffrs workspace
    Workspace(WorkspaceManifest),
}

impl Manifest {
    /// Returns a human friendly representation the current package.
    ///
    /// Intended to be used in error messages on the CLI top level
    pub async fn name() -> Option<String> {
        let manifest = Manifest::load().await.ok()?;

        let cwd = std::env::current_dir().unwrap();

        let name = cwd.file_name()?.to_str();

        match manifest {
            Manifest::Package(p) => Some(p.package?.name.to_string()),
            Manifest::Workspace(_) => name.map(String::from),
        }
    }
    /// Ensures the current directory contains a package manifest, not a workspace
    ///
    /// Returns an error if the manifest is a workspace manifest, otherwise the package manifest
    /// Use this at the beginning of commands that don't support workspaces.
    pub async fn require_package_manifest(path: &PathBuf) -> miette::Result<PackagesManifest> {
        let manifest = Manifest::load_from(path).await?;

        match manifest {
            Manifest::Package(manifest) => Ok(manifest),
            Manifest::Workspace(_) => {
                miette::bail!("A packages manifest is required, but a workspace manifest was found")
            }
        }
    }

    /// Returns the packages manifest if correct type, errs otherwise
    pub fn to_package_manifest(self) -> miette::Result<PackagesManifest> {
        match self {
            Manifest::Package(packages_manifest) => Ok(packages_manifest),
            Manifest::Workspace(_) => {
                miette::bail!("A packages manifest is required, but a workspace manifest was found")
            }
        }
    }
}

#[async_trait]
impl File for Manifest {
    const DEFAULT_PATH: &str = MANIFEST_FILE;

    async fn load_from<P>(path: P) -> miette::Result<Self>
    where
        P: AsRef<Path> + Send + Sync,
    {
        RawManifest::load_from(path).await?.try_into()
    }

    async fn save<P>(&self, path: P) -> miette::Result<()>
    where
        P: AsRef<Path> + Send + Sync,
    {
        RawManifest::from(self.clone()).save(path).await
    }
}

impl FromStr for Manifest {
    type Err = miette::Report;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        input
            .parse::<RawManifest>()
            .into_diagnostic()
            .map(Self::try_from)?
    }
}

impl TryInto<String> for Manifest {
    type Error = toml::ser::Error;

    fn try_into(self) -> Result<String, Self::Error> {
        match self {
            Manifest::Package(p) => p.try_into(),
            Manifest::Workspace(w) => w.try_into(),
        }
    }
}

/// A manifest can define either a package or a workspace
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManifestType {
    /// The Manifest defines a package
    Package,
    /// The Manifest defines a workspace
    Workspace,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::package::PackageType;
    use semver::{Version, VersionReq};
    use std::str::FromStr;

    use crate::package::PackageName;
    use crate::registry::RegistryUri;

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

        let manifest = Manifest::from_str(toml).expect("should parse");
        assert!(matches!(manifest, Manifest::Package(_)));
    }

    #[test]
    fn buffrs_manifest_workspace_from_str() {
        let toml = r#"
                [workspace]
                members = ["pkg1"]
            "#;

        let manifest = Manifest::from_str(toml).expect("should parse");
        assert!(matches!(manifest, Manifest::Workspace(_)));
    }

    #[test]
    fn buffrs_manifest_invalid_mixed() {
        let toml = r#"
                [workspace]
                members = ["pkg1"]

                [dependencies]
            "#;

        let result = Manifest::from_str(toml);
        assert!(result.is_err());
    }

    #[test]
    fn buffrs_manifest_invalid_empty() {
        let result = Manifest::from_str("");
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

        let manifest = Manifest::from_str(toml).expect("should parse");
        let result = manifest.to_package_manifest();
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn buffrs_manifest_to_package_manifest_fails_for_workspace() {
        let toml = r#"
                [workspace]
                members = ["pkg1"]
            "#;

        let manifest = Manifest::from_str(toml).expect("should parse");
        let result = manifest.to_package_manifest();
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

        let manifest = Manifest::from_str(toml).expect("should parse");
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

        let manifest = Manifest::from_str(toml).expect("should parse");
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

    mod edition {
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

    mod dependencies {
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
