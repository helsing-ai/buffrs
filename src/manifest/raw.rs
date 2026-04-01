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
    fmt::{self},
    path::Path,
    str::FromStr,
};

use async_trait::async_trait;
use miette::{Context, IntoDiagnostic, bail, miette};
use serde::{Deserialize, Serialize};
use tokio::fs;

use super::package::{
    Dependency, DependencyManifest, DependencyMap, PackageManifest, PackagesManifest,
};
use super::workspace::{Workspace, WorkspaceManifest};
use super::{CANARY_EDITION, Edition, MANIFEST_FILE, Manifest};
use crate::{
    ManagedFile,
    errors::{DeserializationError, InvalidManifestError, SerializationError, WriteError},
    io::File,
    package::PackageName,
};

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
                        | Edition::Canary12
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

#[async_trait]
impl File for RawManifest {
    const DEFAULT_PATH: &str = MANIFEST_FILE;

    async fn load_from<P>(path: P) -> miette::Result<Self>
    where
        P: AsRef<Path> + Send + Sync,
    {
        let resolved = Self::resolve(path)?;

        let contents = match fs::read_to_string(&resolved).await {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(e).into_diagnostic().wrap_err(miette!(
                    "failed to read non-existent manifest file from `{}`",
                    resolved.display()
                ));
            }
            Err(e) => {
                return Err(e).into_diagnostic().wrap_err(miette!(
                    "failed to read manifest from `{}`",
                    resolved.display()
                ));
            }
        };

        let raw: RawManifest = toml::from_str(&contents)
            .into_diagnostic()
            .wrap_err(DeserializationError(ManagedFile::Manifest))?;

        Ok(raw)
    }

    async fn save_to<P>(&self, path: P) -> miette::Result<()>
    where
        P: AsRef<Path> + Send + Sync,
    {
        let resolved = Self::resolve(path)?;

        fs::write(
            resolved,
            toml::to_string(&self)
                .into_diagnostic()
                .wrap_err(SerializationError(ManagedFile::Manifest))?
                .into_bytes(),
        )
        .await
        .into_diagnostic()
        .wrap_err(WriteError(MANIFEST_FILE))
    }
}

impl From<WorkspaceManifest> for RawManifest {
    fn from(workspace_manifest: WorkspaceManifest) -> Self {
        RawManifest::Canary {
            package: None,
            dependencies: None,
            workspace: Some(workspace_manifest.workspace),
        }
    }
}

impl From<Manifest> for RawManifest {
    fn from(manifest: Manifest) -> Self {
        match manifest {
            Manifest::Package(package_manifest) => package_manifest.into(),
            Manifest::Workspace(workspace_manifest) => workspace_manifest.into(),
        }
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
            workspace: None,
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

impl TryFrom<RawManifest> for Manifest {
    type Error = miette::Report;

    fn try_from(raw: RawManifest) -> Result<Manifest, Self::Error> {
        let dependencies = raw.dependencies_as_vec();
        let workspace = raw.workspace().cloned();
        let package = raw.package().cloned();

        match (&dependencies, &workspace) {
            (&Some(_), &Some(_)) => Err(miette!(
                "manifest cannot have both dependencies and workspace sections"
            ))
            .wrap_err(InvalidManifestError(ManagedFile::Manifest)),
            (None, None) => {
                if package.is_some() {
                    Ok(Manifest::Package(raw.try_into()?))
                } else {
                    Err(miette!(
                        "manifest should have either a package or a workspace section"
                    ))
                    .wrap_err(InvalidManifestError(ManagedFile::Manifest))
                }
            }
            (&Some(_), None) => Ok(Manifest::Package(raw.try_into()?)),
            (None, &Some(_)) => Ok(Manifest::Workspace(raw.try_into()?)),
        }
    }
}

#[cfg(test)]
mod tests {
    use semver::{Version, VersionReq};

    use super::*;

    use crate::manifest::*;
    use crate::package::*;
    use crate::registry::*;

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

        let manifest = Manifest::from_str(manifest).expect("should be valid manifest");
        let cloned_raw_manifest_str = toml::to_string(&RawManifest::from(manifest.clone()))
            .expect("should be convertible to str");
        let raw_manifest_str =
            toml::to_string(&RawManifest::from(manifest)).expect("should be convertible to str");

        assert!(cloned_raw_manifest_str.contains("edition"));
        assert_eq!(cloned_raw_manifest_str, raw_manifest_str);
    }

    mod detection {
        use super::*;

        #[test]
        fn manifest_type_package() {
            let raw = RawManifest::Canary {
                package: None,
                dependencies: Some(HashMap::new()),
                workspace: None,
            };
            let result = Manifest::try_from(raw);
            assert!(result.is_ok());
            assert!(matches!(result.unwrap(), Manifest::Package(_)));
        }

        #[test]
        fn manifest_type_workspace() {
            let raw = RawManifest::Canary {
                package: None,
                dependencies: None,
                workspace: Some(Workspace {
                    members: vec!["pkg1".to_string()],
                    exclude: None,
                }),
            };
            let result = Manifest::try_from(raw);
            assert!(result.is_ok());
            assert!(matches!(result.unwrap(), Manifest::Workspace(_)));
        }

        #[test]
        fn manifest_type_both_dependencies_and_workspace_errors() {
            let raw = RawManifest::Canary {
                package: None,
                dependencies: Some(vec![].into_iter().collect()),
                workspace: Some(Workspace {
                    members: vec!["pkg1".to_string()],
                    exclude: None,
                }),
            };
            let result = Manifest::try_from(raw);
            assert!(result.is_err());
            let err_msg = result.unwrap_err().to_string();
            assert!(err_msg.contains("manifest") && err_msg.contains("invalid"));
        }

        #[test]
        fn manifest_type_package_with_no_dependencies() {
            let raw = RawManifest::Canary {
                package: Some(PackageManifest {
                    kind: PackageType::Lib,
                    name: PackageName::new("test").unwrap(),
                    version: Version::new(1, 0, 0),
                    description: None,
                }),
                dependencies: None,
                workspace: None,
            };
            let result = Manifest::try_from(raw);
            assert!(result.is_ok());
            assert!(matches!(result.unwrap(), Manifest::Package(_)));
        }

        #[test]
        fn manifest_type_neither_dependencies_nor_workspace_nor_package_errors() {
            let raw = RawManifest::Canary {
                package: None,
                dependencies: None,
                workspace: None,
            };
            let result = Manifest::try_from(raw);
            assert!(result.is_err());
        }
    }
}
