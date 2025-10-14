use crate::manifest::{
    Dependency, DependencyManifest, LocalDependencyManifest, MANIFEST_FILE, Manifest,
};
use crate::package::{PackageName, PackageType};
use crate::registry::RegistryUri;
use async_recursion::async_recursion;
use miette::{Diagnostic, bail};
use semver::VersionReq;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use thiserror::Error;

/// Models the source of a dependency
#[derive(Debug, Clone)]
pub enum DependencySource {
    /// A local dependencies, expressed by it's path
    Local { path: PathBuf },
    /// A remote dependencies, expressed by it's repo & registry
    Remote {
        registry: RegistryUri,
        repository: String,
    },
}

/// Represents metadata about a dependency in the graph
#[derive(Debug, Clone)]
pub struct DependencyNode {
    /// The package name
    pub name: PackageName,
    /// Package type (api or lib)
    pub package_type: Option<PackageType>,
    /// Where this dependency comes from
    pub source: DependencySource,
    /// Packages that this package depends on
    pub dependencies: Vec<PackageName>,
    /// Version requirement
    pub version: VersionReq,
}

// Maps a package name to metadata describing the package
pub type MetadataMap = HashMap<PackageName, DependencyNode>;

/// A dependency graph containing nodes and edges
#[derive(Debug)]
pub struct DependencyGraph {
    /// Map of package names to their metadata
    nodes: MetadataMap,
}

impl DependencyGraph {
    /// Build a dependency graph from a manifest
    ///
    /// This is a pure graph construction - no I/O operations like downloading or installing
    pub async fn build(manifest: &Manifest, base_path: &PathBuf) -> miette::Result<Self> {
        let mut builder = GraphBuilder::new(base_path.clone());

        // Get the parent package type from the manifest
        let parent_package_type = manifest.package.as_ref().map(|p| p.kind);

        // Add root dependencies
        for dependency in manifest.dependencies.iter().flatten() {
            builder
                .add_dependency(dependency, parent_package_type)
                .await?;
        }

        Ok(Self {
            nodes: builder.nodes,
        })
    }

    /// Perform topological sort on the dependency graph
    ///
    /// Returns packages in installation order (dependencies before dependents)
    /// Detects cycles and returns an error if found
    ///
    /// Implements Kahn's algorithm
    pub fn topological_sort(&self) -> Result<Vec<PackageName>, DependencyError> {
        let mut in_degree: HashMap<PackageName, usize> = HashMap::new();
        let mut adj_list: HashMap<PackageName, Vec<PackageName>> = HashMap::new();

        // Initialize in-degree and adjacency list
        // For topological sort: process dependencies before dependents
        // in_degree[A] = number of packages A depends on
        // adj_list[A] = list of packages that depend on A
        for (name, node) in &self.nodes {
            in_degree.entry(name.clone()).or_insert(0);

            for dep in &node.dependencies {
                // name depends on dep, so increment names in-degree
                *in_degree.entry(name.clone()).or_insert(0) += 1;
                // Add name to dep's dependents list
                adj_list
                    .entry(dep.clone())
                    .or_insert_with(Vec::new)
                    .push(name.clone());
            }
        }

        // Initialize queue with V that don't depend on any other node.
        // At least one such E needs exist, otherwise a cycle exists
        let mut queue: VecDeque<PackageName> = in_degree
            .iter()
            .filter(|&(_, degree)| *degree == 0)
            .map(|(name, _)| name.clone())
            .collect();

        let mut sorted = Vec::new();

        // Process queue, add process V to sorted list
        while let Some(node) = queue.pop_front() {
            sorted.push(node.clone());

            if let Some(neighbors) = adj_list.get(&node) {
                for neighbor in neighbors {
                    if let Some(degree) = in_degree.get_mut(neighbor) {
                        *degree -= 1;

                        let no_incoming_edges = *degree == 0;
                        if no_incoming_edges {
                            queue.push_back(neighbor.clone());
                        }
                    }
                }
            }
        }

        // Cycles should not happen since we check during graph constructon, but
        if sorted.len() != self.nodes.len() {
            let unprocessed: Vec<_> = self
                .nodes
                .keys()
                .filter(|name| !sorted.contains(name))
                .map(|n| n.to_string())
                .collect();

            return Err(DependencyError::CircularDependency(
                unprocessed.join(" -> "),
            ));
        }

        Ok(sorted)
    }
}

/// Internal builder for constructing the dependency graph
struct GraphBuilder {
    nodes: HashMap<PackageName, DependencyNode>,
    base_path: PathBuf,
    /// Track which packages we're currently visiting to detect cycles during construction
    visiting: HashSet<PackageName>,
}

impl GraphBuilder {
    fn new(base_path: PathBuf) -> Self {
        Self {
            nodes: HashMap::new(),
            base_path,
            visiting: HashSet::new(),
        }
    }

    #[async_recursion]
    async fn add_dependency(
        &mut self,
        dependency: &Dependency,
        parent_type: Option<PackageType>,
    ) -> miette::Result<()> {
        let package_name = &dependency.package;

        // Check for cycle during traversal
        if self.visiting.contains(package_name) {
            bail!(DependencyError::CircularDependency(format!(
                "detected while processing {}",
                package_name
            )));
        }

        // If already processed, just validate compatibility
        if let Some(existing) = self.nodes.get(package_name) {
            self.validate_compatibility(dependency, existing)?;
            return Ok(());
        }

        // Mark as visiting
        self.visiting.insert(package_name.clone());

        match &dependency.manifest {
            DependencyManifest::Local(local) => {
                self.add_local_dependency(dependency, local, parent_type)
                    .await?;
            }
            DependencyManifest::Remote(remote) => {
                self.add_remote_dependency(dependency, remote, parent_type)
                    .await?;
            }
        }

        // Unmark as visiting
        self.visiting.remove(package_name);

        Ok(())
    }

    async fn add_local_dependency(
        &mut self,
        dependency: &Dependency,
        local_manifest: &LocalDependencyManifest,
        parent_type: Option<PackageType>,
    ) -> miette::Result<()> {
        let resolved_path = self.base_path.join(&local_manifest.path);
        let manifest_path = resolved_path.join(MANIFEST_FILE);

        let manifest = Manifest::try_read_from(&manifest_path).await?;
        let package_type = manifest.package.as_ref().map(|p| p.kind);

        // Validate package type constraint
        if let (Some(PackageType::Lib), Some(PackageType::Api)) = (parent_type, package_type) {
            bail!(DependencyError::InvalidPackageTypeDependency {
                parent: PackageName::unchecked("parent"), // TODO: thread parent name
                dependency: dependency.package.clone(),
            });
        }

        let sub_dependencies: Vec<PackageName> = manifest.get_dependency_package_names();

        // Add node
        self.nodes.insert(
            dependency.package.clone(),
            DependencyNode {
                name: dependency.package.clone(),
                package_type,
                source: DependencySource::Local {
                    path: resolved_path.clone(),
                },
                dependencies: sub_dependencies.clone(),
                version: VersionReq::STAR,
            },
        );

        // Recursively process dependencies with the new base path
        for sub_dep in manifest.dependencies.unwrap_or_default() {
            // We need to update the base path for sub-dependencies
            let old_base = self.base_path.clone();
            self.base_path = resolved_path.clone();
            self.add_dependency(&sub_dep, package_type).await?;
            self.base_path = old_base;
        }

        Ok(())
    }

    async fn add_remote_dependency(
        &mut self,
        dependency: &Dependency,
        _remote_manifest: &crate::manifest::RemoteDependencyManifest,
        _parent_type: Option<PackageType>,
    ) -> miette::Result<()> {
        // For now, just create a placeholder node
        // In the real implementation, we'd fetch metadata without downloading the full package
        self.nodes.insert(
            dependency.package.clone(),
            DependencyNode {
                name: dependency.package.clone(),
                package_type: None, // Would need to fetch this
                source: DependencySource::Remote {
                    registry: _remote_manifest.registry.clone(),
                    repository: _remote_manifest.repository.clone(),
                },
                dependencies: vec![], // Would need to fetch this
                version: _remote_manifest.version.clone(),
            },
        );

        Ok(())
    }

    fn validate_compatibility(
        &self,
        dependency: &Dependency,
        existing: &DependencyNode,
    ) -> miette::Result<()> {
        // Check for local/remote conflicts
        match (&dependency.manifest, &existing.source) {
            (DependencyManifest::Local(_), DependencySource::Remote { .. }) => {
                bail!(DependencyError::LocalRemoteConflict {
                    local_pkg: dependency.package.clone(),
                    requester: PackageName::unchecked("unknown"), // TODO: thread requester
                });
            }
            (DependencyManifest::Remote(_), DependencySource::Local { .. }) => {
                bail!(DependencyError::LocalRemoteConflict {
                    local_pkg: dependency.package.clone(),
                    requester: PackageName::unchecked("unknown"),
                });
            }
            _ => {}
        }

        Ok(())
    }
}

#[derive(Error, Diagnostic, Debug)]
pub enum DependencyError {
    #[error(
        "local dependency {local_pkg} conflicts with remote dependency required by {requester}"
    )]
    LocalRemoteConflict {
        local_pkg: PackageName,
        requester: PackageName,
    },

    #[error("package of type lib cannot depend on package of type api: {parent} -> {dependency}")]
    InvalidPackageTypeDependency {
        parent: PackageName,
        dependency: PackageName,
    },

    #[error("circular dependency detected: {0}")]
    CircularDependency(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::{Manifest, PackageManifest};
    use semver::Version;
    use tempfile::TempDir;

    fn create_test_manifest(
        name: &str,
        package_type: PackageType,
        dependencies: Vec<Dependency>,
    ) -> Manifest {
        Manifest::builder()
            .package(PackageManifest {
                kind: package_type,
                name: name.parse().expect("valid package name"),
                version: Version::new(0, 1, 0),
                description: None,
            })
            .dependencies(dependencies)
            .build()
    }

    #[tokio::test]
    async fn test_empty_graph() {
        let manifest = create_test_manifest("test-package", PackageType::Lib, vec![]);
        let temp_dir = TempDir::new().expect("create temp dir");

        let graph = DependencyGraph::build(&manifest, &temp_dir.path().to_path_buf())
            .await
            .expect("build graph");

        assert_eq!(graph.nodes.len(), 0);
    }

    #[tokio::test]
    async fn test_single_local_dependency() {
        let temp_dir = TempDir::new().expect("create temp dir");
        let lib_dir = temp_dir.path().join("lib-package");
        std::fs::create_dir(&lib_dir).expect("create lib dir");
        std::fs::create_dir_all(lib_dir.join("proto")).expect("create proto dir");

        // Create a lib package with no dependencies
        let lib_manifest = create_test_manifest("lib-package", PackageType::Lib, vec![]);
        lib_manifest
            .write_at(&lib_dir)
            .await
            .expect("write lib manifest");

        // Create an API package that depends on the lib
        let api_manifest = Manifest::builder()
            .package(PackageManifest {
                kind: PackageType::Api,
                name: "api-package".parse().expect("valid package name"),
                version: Version::new(0, 1, 0),
                description: None,
            })
            .dependencies(vec![Dependency {
                package: "lib-package".parse().expect("valid package name"),
                manifest: LocalDependencyManifest {
                    path: lib_dir.clone(),
                }
                .into(),
            }])
            .build();

        let graph = DependencyGraph::build(&api_manifest, &temp_dir.path().to_path_buf())
            .await
            .expect("build graph");

        assert_eq!(graph.nodes.len(), 1);
        let lib_node = graph
            .nodes
            .get(&"lib-package".parse().expect("valid package name"))
            .expect("lib node exists");
        assert_eq!(lib_node.dependencies.len(), 0);
        assert!(matches!(lib_node.package_type, Some(PackageType::Lib)));
    }

    #[tokio::test]
    async fn test_transitive_dependencies() {
        let temp_dir = TempDir::new().expect("create temp dir");

        // Create lib2 (no dependencies)
        let lib2_dir = temp_dir.path().join("lib2");
        std::fs::create_dir(&lib2_dir).expect("create lib2 dir");
        std::fs::create_dir_all(lib2_dir.join("proto")).expect("create proto dir");
        let lib2_manifest = create_test_manifest("lib2", PackageType::Lib, vec![]);
        lib2_manifest
            .write_at(&lib2_dir)
            .await
            .expect("write lib2 manifest");

        // Create lib1 (depends on lib2)
        let lib1_dir = temp_dir.path().join("lib1");
        std::fs::create_dir(&lib1_dir).expect("create lib1 dir");
        std::fs::create_dir_all(lib1_dir.join("proto")).expect("create proto dir");
        let lib1_manifest = Manifest::builder()
            .package(PackageManifest {
                kind: PackageType::Lib,
                name: "lib1".parse().expect("valid package name"),
                version: Version::new(0, 1, 0),
                description: None,
            })
            .dependencies(vec![Dependency {
                package: "lib2".parse().expect("valid package name"),
                manifest: LocalDependencyManifest {
                    path: PathBuf::from("../lib2"),
                }
                .into(),
            }])
            .build();
        lib1_manifest
            .write_at(&lib1_dir)
            .await
            .expect("write lib1 manifest");

        // Create api (depends on lib1)
        let api_manifest = Manifest::builder()
            .package(PackageManifest {
                kind: PackageType::Api,
                name: "api".parse().expect("valid package name"),
                version: Version::new(0, 1, 0),
                description: None,
            })
            .dependencies(vec![Dependency {
                package: "lib1".parse().expect("valid package name"),
                manifest: LocalDependencyManifest {
                    path: lib1_dir.clone(),
                }
                .into(),
            }])
            .build();

        let graph = DependencyGraph::build(&api_manifest, &temp_dir.path().to_path_buf())
            .await
            .expect("build graph");

        // Should have both lib1 and lib2 in the graph
        assert_eq!(graph.nodes.len(), 2);

        let lib1_node = graph
            .nodes
            .get(&"lib1".parse().expect("valid package name"))
            .expect("lib1 node exists");
        assert_eq!(lib1_node.dependencies.len(), 1);
        assert!(
            lib1_node
                .dependencies
                .contains(&"lib2".parse().expect("valid package name"))
        );

        let lib2_node = graph
            .nodes
            .get(&"lib2".parse().expect("valid package name"))
            .expect("lib2 node exists");
        assert_eq!(lib2_node.dependencies.len(), 0);
    }

    #[tokio::test]
    async fn test_lib_cannot_depend_on_api() {
        let temp_dir = TempDir::new().expect("create temp dir");
        let api_dir = temp_dir.path().join("api-package");
        std::fs::create_dir(&api_dir).expect("create dir");
        std::fs::create_dir_all(api_dir.join("proto")).expect("create proto dir");

        // Create an API package
        let api_manifest = create_test_manifest("api-package", PackageType::Api, vec![]);
        api_manifest
            .write_at(&api_dir)
            .await
            .expect("write manifest");

        // Create a lib package that tries to depend on the API
        let lib_manifest = Manifest::builder()
            .package(PackageManifest {
                kind: PackageType::Lib,
                name: "lib-package".parse().expect("valid package name"),
                version: Version::new(0, 1, 0),
                description: None,
            })
            .dependencies(vec![Dependency {
                package: "api-package".parse().expect("valid package name"),
                manifest: LocalDependencyManifest {
                    path: api_dir.clone(),
                }
                .into(),
            }])
            .build();

        let result = DependencyGraph::build(&lib_manifest, &temp_dir.path().to_path_buf()).await;

        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("lib cannot depend")
                || err_msg.contains("InvalidPackageTypeDependency"),
            "Error message should mention lib/api restriction, got: {}",
            err_msg
        );
    }

    #[tokio::test]
    async fn test_api_can_depend_on_lib() {
        let temp_dir = TempDir::new().expect("create temp dir");
        let lib_dir = temp_dir.path().join("lib-package");
        std::fs::create_dir(&lib_dir).expect("create dir");
        std::fs::create_dir_all(lib_dir.join("proto")).expect("create proto dir");

        let lib_manifest = create_test_manifest("lib-package", PackageType::Lib, vec![]);
        lib_manifest
            .write_at(&lib_dir)
            .await
            .expect("write manifest");

        let api_manifest = Manifest::builder()
            .package(PackageManifest {
                kind: PackageType::Api,
                name: "api-package".parse().expect("valid package name"),
                version: Version::new(0, 1, 0),
                description: None,
            })
            .dependencies(vec![Dependency {
                package: "lib-package".parse().expect("valid package name"),
                manifest: LocalDependencyManifest {
                    path: lib_dir.clone(),
                }
                .into(),
            }])
            .build();

        let result = DependencyGraph::build(&api_manifest, &temp_dir.path().to_path_buf()).await;
        assert!(result.is_ok(), "API should be able to depend on lib");
    }

    #[tokio::test]
    async fn test_circular_dependency_direct() {
        let temp_dir = TempDir::new().expect("create temp dir");

        // Create pkg1 directory
        let pkg1_dir = temp_dir.path().join("pkg1");
        std::fs::create_dir(&pkg1_dir).expect("create dir");
        std::fs::create_dir_all(pkg1_dir.join("proto")).expect("create proto dir");

        // Create pkg2 directory
        let pkg2_dir = temp_dir.path().join("pkg2");
        std::fs::create_dir(&pkg2_dir).expect("create dir");
        std::fs::create_dir_all(pkg2_dir.join("proto")).expect("create proto dir");

        // Create pkg2 manifest (depends on pkg1 using absolute path to temp_dir/pkg1)
        let pkg2_manifest = Manifest::builder()
            .package(PackageManifest {
                kind: PackageType::Lib,
                name: "pkg2".parse().expect("valid package name"),
                version: Version::new(0, 1, 0),
                description: None,
            })
            .dependencies(vec![Dependency {
                package: "pkg1".parse().expect("valid package name"),
                manifest: LocalDependencyManifest {
                    path: pkg1_dir.clone(),
                }
                .into(),
            }])
            .build();
        pkg2_manifest
            .write_at(&pkg2_dir)
            .await
            .expect("write manifest");

        // Create pkg1 manifest (depends on pkg2 - circular!)
        let pkg1_manifest = Manifest::builder()
            .package(PackageManifest {
                kind: PackageType::Lib,
                name: "pkg1".parse().expect("valid package name"),
                version: Version::new(0, 1, 0),
                description: None,
            })
            .dependencies(vec![Dependency {
                package: "pkg2".parse().expect("valid package name"),
                manifest: LocalDependencyManifest {
                    path: pkg2_dir.clone(),
                }
                .into(),
            }])
            .build();
        pkg1_manifest
            .write_at(&pkg1_dir)
            .await
            .expect("write manifest");

        // Start building from pkg1's directory
        let result = DependencyGraph::build(&pkg1_manifest, &pkg1_dir).await;

        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("circular") || err_msg.contains("CircularDependency"),
            "Error should mention circular dependency, got: {}",
            err_msg
        );
    }

    #[tokio::test]
    async fn test_circular_dependency_indirect() {
        let temp_dir = TempDir::new().expect("create temp dir");

        // Create pkg3 (no dependencies initially)
        let pkg3_dir = temp_dir.path().join("pkg3");
        std::fs::create_dir(&pkg3_dir).expect("create dir");
        std::fs::create_dir_all(pkg3_dir.join("proto")).expect("create proto dir");

        // Create pkg2 (depends on pkg3)
        let pkg2_dir = temp_dir.path().join("pkg2");
        std::fs::create_dir(&pkg2_dir).expect("create dir");
        std::fs::create_dir_all(pkg2_dir.join("proto")).expect("create proto dir");

        // Create pkg1 (depends on pkg2)
        let pkg1_dir = temp_dir.path().join("pkg1");
        std::fs::create_dir(&pkg1_dir).expect("create dir");
        std::fs::create_dir_all(pkg1_dir.join("proto")).expect("create proto dir");

        // pkg3 depends on pkg1 to create cycle: pkg1 -> pkg2 -> pkg3 -> pkg1
        let pkg3_manifest = Manifest::builder()
            .package(PackageManifest {
                kind: PackageType::Lib,
                name: "pkg3".parse().expect("valid package name"),
                version: Version::new(0, 1, 0),
                description: None,
            })
            .dependencies(vec![Dependency {
                package: "pkg1".parse().expect("valid package name"),
                manifest: LocalDependencyManifest {
                    path: pkg1_dir.clone(),
                }
                .into(),
            }])
            .build();
        pkg3_manifest
            .write_at(&pkg3_dir)
            .await
            .expect("write manifest");

        let pkg2_manifest = Manifest::builder()
            .package(PackageManifest {
                kind: PackageType::Lib,
                name: "pkg2".parse().expect("valid package name"),
                version: Version::new(0, 1, 0),
                description: None,
            })
            .dependencies(vec![Dependency {
                package: "pkg3".parse().expect("valid package name"),
                manifest: LocalDependencyManifest {
                    path: pkg3_dir.clone(),
                }
                .into(),
            }])
            .build();
        pkg2_manifest
            .write_at(&pkg2_dir)
            .await
            .expect("write manifest");

        let pkg1_manifest = Manifest::builder()
            .package(PackageManifest {
                kind: PackageType::Lib,
                name: "pkg1".parse().expect("valid package name"),
                version: Version::new(0, 1, 0),
                description: None,
            })
            .dependencies(vec![Dependency {
                package: "pkg2".parse().expect("valid package name"),
                manifest: LocalDependencyManifest {
                    path: pkg2_dir.clone(),
                }
                .into(),
            }])
            .build();
        pkg1_manifest
            .write_at(&pkg1_dir)
            .await
            .expect("write manifest");

        // Start building from pkg1's directory
        let result = DependencyGraph::build(&pkg1_manifest, &pkg1_dir).await;

        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("circular") || err_msg.contains("CircularDependency"),
            "Error should mention circular dependency, got: {}",
            err_msg
        );
    }

    #[tokio::test]
    async fn test_diamond_dependency() {
        let temp_dir = TempDir::new().expect("create temp dir");

        // Create common (no dependencies)
        let common_dir = temp_dir.path().join("common");
        std::fs::create_dir(&common_dir).expect("create dir");
        std::fs::create_dir_all(common_dir.join("proto")).expect("create proto dir");
        let common_manifest = create_test_manifest("common", PackageType::Lib, vec![]);
        common_manifest
            .write_at(&common_dir)
            .await
            .expect("write manifest");

        // Create lib1 (depends on common)
        let lib1_dir = temp_dir.path().join("lib1");
        std::fs::create_dir(&lib1_dir).expect("create dir");
        std::fs::create_dir_all(lib1_dir.join("proto")).expect("create proto dir");
        let lib1_manifest = Manifest::builder()
            .package(PackageManifest {
                kind: PackageType::Lib,
                name: "lib1".parse().expect("valid package name"),
                version: Version::new(0, 1, 0),
                description: None,
            })
            .dependencies(vec![Dependency {
                package: "common".parse().expect("valid package name"),
                manifest: LocalDependencyManifest {
                    path: PathBuf::from("../common"),
                }
                .into(),
            }])
            .build();
        lib1_manifest
            .write_at(&lib1_dir)
            .await
            .expect("write manifest");

        // Create lib2 (depends on common)
        let lib2_dir = temp_dir.path().join("lib2");
        std::fs::create_dir(&lib2_dir).expect("create dir");
        std::fs::create_dir_all(lib2_dir.join("proto")).expect("create proto dir");
        let lib2_manifest = Manifest::builder()
            .package(PackageManifest {
                kind: PackageType::Lib,
                name: "lib2".parse().expect("valid package name"),
                version: Version::new(0, 1, 0),
                description: None,
            })
            .dependencies(vec![Dependency {
                package: "common".parse().expect("valid package name"),
                manifest: LocalDependencyManifest {
                    path: PathBuf::from("../common"),
                }
                .into(),
            }])
            .build();
        lib2_manifest
            .write_at(&lib2_dir)
            .await
            .expect("write manifest");

        // Create api (depends on both lib1 and lib2, creating diamond)
        let api_manifest = Manifest::builder()
            .package(PackageManifest {
                kind: PackageType::Api,
                name: "api".parse().expect("valid package name"),
                version: Version::new(0, 1, 0),
                description: None,
            })
            .dependencies(vec![
                Dependency {
                    package: "lib1".parse().expect("valid package name"),
                    manifest: LocalDependencyManifest {
                        path: lib1_dir.clone(),
                    }
                    .into(),
                },
                Dependency {
                    package: "lib2".parse().expect("valid package name"),
                    manifest: LocalDependencyManifest {
                        path: lib2_dir.clone(),
                    }
                    .into(),
                },
            ])
            .build();

        let graph = DependencyGraph::build(&api_manifest, &temp_dir.path().to_path_buf())
            .await
            .expect("build graph");

        // Should have 3 packages (common should appear only once despite being depended on twice)
        assert_eq!(graph.nodes.len(), 3);
        assert!(
            graph
                .nodes
                .contains_key(&"common".parse().expect("valid package name"))
        );
        assert!(
            graph
                .nodes
                .contains_key(&"lib1".parse().expect("valid package name"))
        );
        assert!(
            graph
                .nodes
                .contains_key(&"lib2".parse().expect("valid package name"))
        );
    }

    #[tokio::test]
    async fn test_multiple_dependencies_from_single_package() {
        let temp_dir = TempDir::new().expect("create temp dir");

        // Create lib1
        let lib1_dir = temp_dir.path().join("lib1");
        std::fs::create_dir(&lib1_dir).expect("create dir");
        std::fs::create_dir_all(lib1_dir.join("proto")).expect("create proto dir");
        let lib1_manifest = create_test_manifest("lib1", PackageType::Lib, vec![]);
        lib1_manifest
            .write_at(&lib1_dir)
            .await
            .expect("write manifest");

        // Create lib2
        let lib2_dir = temp_dir.path().join("lib2");
        std::fs::create_dir(&lib2_dir).expect("create dir");
        std::fs::create_dir_all(lib2_dir.join("proto")).expect("create proto dir");
        let lib2_manifest = create_test_manifest("lib2", PackageType::Lib, vec![]);
        lib2_manifest
            .write_at(&lib2_dir)
            .await
            .expect("write manifest");

        // Create lib3
        let lib3_dir = temp_dir.path().join("lib3");
        std::fs::create_dir(&lib3_dir).expect("create dir");
        std::fs::create_dir_all(lib3_dir.join("proto")).expect("create proto dir");
        let lib3_manifest = create_test_manifest("lib3", PackageType::Lib, vec![]);
        lib3_manifest
            .write_at(&lib3_dir)
            .await
            .expect("write manifest");

        // Create api that depends on all three
        let api_manifest = Manifest::builder()
            .package(PackageManifest {
                kind: PackageType::Api,
                name: "api".parse().expect("valid package name"),
                version: Version::new(0, 1, 0),
                description: None,
            })
            .dependencies(vec![
                Dependency {
                    package: "lib1".parse().expect("valid package name"),
                    manifest: LocalDependencyManifest {
                        path: lib1_dir.clone(),
                    }
                    .into(),
                },
                Dependency {
                    package: "lib2".parse().expect("valid package name"),
                    manifest: LocalDependencyManifest {
                        path: lib2_dir.clone(),
                    }
                    .into(),
                },
                Dependency {
                    package: "lib3".parse().expect("valid package name"),
                    manifest: LocalDependencyManifest {
                        path: lib3_dir.clone(),
                    }
                    .into(),
                },
            ])
            .build();

        let graph = DependencyGraph::build(&api_manifest, &temp_dir.path().to_path_buf())
            .await
            .expect("build graph");

        assert_eq!(graph.nodes.len(), 3);
        assert!(
            graph
                .nodes
                .contains_key(&"lib1".parse().expect("valid package name"))
        );
        assert!(
            graph
                .nodes
                .contains_key(&"lib2".parse().expect("valid package name"))
        );
        assert!(
            graph
                .nodes
                .contains_key(&"lib3".parse().expect("valid package name"))
        );
    }

    #[tokio::test]
    async fn test_local_remote_conflict() {
        let temp_dir = TempDir::new().expect("create temp dir");
        let lib_dir = temp_dir.path().join("lib-package");
        std::fs::create_dir(&lib_dir).expect("create dir");
        std::fs::create_dir_all(lib_dir.join("proto")).expect("create proto dir");

        let lib_manifest = create_test_manifest("lib-package", PackageType::Lib, vec![]);
        lib_manifest
            .write_at(&lib_dir)
            .await
            .expect("write manifest");

        // Create a manifest with both local and remote dependency on same package
        // This is a bit contrived but tests the validation logic
        let api_manifest = Manifest::builder()
            .package(PackageManifest {
                kind: PackageType::Api,
                name: "api-package".parse().expect("valid package name"),
                version: Version::new(0, 1, 0),
                description: None,
            })
            .dependencies(vec![
                Dependency {
                    package: "lib-package".parse().expect("valid package name"),
                    manifest: LocalDependencyManifest {
                        path: lib_dir.clone(),
                    }
                    .into(),
                },
                Dependency::new(
                    "https://registry.example.com"
                        .parse()
                        .expect("valid package name"),
                    "test-repo".to_string(),
                    "lib-package".parse().expect("valid package name"),
                    VersionReq::parse("=0.1.0").expect("valid version"),
                ),
            ])
            .build();

        let result = DependencyGraph::build(&api_manifest, &temp_dir.path().to_path_buf()).await;

        // Should detect local/remote conflict
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("conflict") || err_msg.contains("LocalRemoteConflict"),
            "Error should mention local/remote conflict, got: {}",
            err_msg
        );
    }

    #[tokio::test]
    async fn test_relative_path_resolution() {
        let temp_dir = TempDir::new().expect("create temp dir");

        // Create nested structure: temp_dir/subdir/lib1
        let subdir = temp_dir.path().join("subdir");
        std::fs::create_dir(&subdir).expect("create dir");

        let lib1_dir = subdir.join("lib1");
        std::fs::create_dir(&lib1_dir).expect("create dir");
        std::fs::create_dir_all(lib1_dir.join("proto")).expect("create proto dir");
        let lib1_manifest = create_test_manifest("lib1", PackageType::Lib, vec![]);
        lib1_manifest
            .write_at(&lib1_dir)
            .await
            .expect("write manifest");

        // Create api at temp_dir/api that uses relative path to lib1
        let api_dir = temp_dir.path().join("api");
        std::fs::create_dir(&api_dir).expect("create dir");
        std::fs::create_dir_all(api_dir.join("proto")).expect("create proto dir");

        let api_manifest = Manifest::builder()
            .package(PackageManifest {
                kind: PackageType::Api,
                name: "api".parse().expect("valid package name"),
                version: Version::new(0, 1, 0),
                description: None,
            })
            .dependencies(vec![Dependency {
                package: "lib1".parse().expect("valid package name"),
                manifest: LocalDependencyManifest {
                    path: PathBuf::from("subdir/lib1"),
                }
                .into(),
            }])
            .build();

        let graph = DependencyGraph::build(&api_manifest, &temp_dir.path().to_path_buf())
            .await
            .expect("build graph");

        assert_eq!(graph.nodes.len(), 1);
        let lib1_node = graph
            .nodes
            .get(&"lib1".parse().expect("valid package name"))
            .expect("node exists");

        if let DependencySource::Local { path } = &lib1_node.source {
            assert!(path.ends_with("subdir/lib1"));
        } else {
            panic!("Expected local dependency source");
        }
    }

    // Topological Sort Tests
    // These tests verify correct ordering without re-testing graph construction

    /// Helper function to manually construct a DependencyGraph for testing topological sort
    fn build_test_graph(nodes: Vec<(PackageName, Vec<PackageName>)>) -> DependencyGraph {
        let mut graph_nodes = HashMap::new();

        for (name, dependencies) in nodes {
            graph_nodes.insert(
                name.clone(),
                DependencyNode {
                    name: name.clone(),
                    package_type: Some(PackageType::Lib),
                    source: DependencySource::Local {
                        path: PathBuf::from("/tmp"),
                    },
                    dependencies,
                    version: VersionReq::STAR,
                },
            );
        }

        DependencyGraph { nodes: graph_nodes }
    }

    #[test]
    fn test_topo_sort_linear_chain() {
        // Graph: a -> b -> c
        // Expected order: c, b, a (dependencies before dependents)
        let graph = build_test_graph(vec![
            ("c".parse().expect("valid package name"), vec![]),
            (
                "b".parse().expect("valid package name"),
                vec!["c".parse().expect("valid package name")],
            ),
            (
                "a".parse().expect("valid package name"),
                vec!["b".parse().expect("valid package name")],
            ),
        ]);

        let sorted = graph.topological_sort().expect("sort should succeed");

        assert_eq!(sorted.len(), 3);

        // Find positions
        let pos_a = sorted
            .iter()
            .position(|n| n.to_string() == "a")
            .expect("a in sorted");
        let pos_b = sorted
            .iter()
            .position(|n| n.to_string() == "b")
            .expect("b in sorted");
        let pos_c = sorted
            .iter()
            .position(|n| n.to_string() == "c")
            .expect("c in sorted");

        // c must come before b, b must come before a
        assert!(pos_c < pos_b, "c should come before b");
        assert!(pos_b < pos_a, "b should come before a");
    }

    #[test]
    fn test_topo_sort_diamond() {
        // Graph: d <- b <- a
        //        d <- c <-/
        // Expected: d before both b and c, b and c before a
        let graph = build_test_graph(vec![
            ("d".parse().expect("valid package name"), vec![]),
            (
                "b".parse().expect("valid package name"),
                vec!["d".parse().expect("valid package name")],
            ),
            (
                "c".parse().expect("valid package name"),
                vec!["d".parse().expect("valid package name")],
            ),
            (
                "a".parse().expect("valid package name"),
                vec![
                    "b".parse().expect("valid package name"),
                    "c".parse().expect("valid package name"),
                ],
            ),
        ]);

        let sorted = graph.topological_sort().expect("sort should succeed");

        assert_eq!(sorted.len(), 4);

        let pos_a = sorted
            .iter()
            .position(|n| n.to_string() == "a")
            .expect("a in sorted");
        let pos_b = sorted
            .iter()
            .position(|n| n.to_string() == "b")
            .expect("b in sorted");
        let pos_c = sorted
            .iter()
            .position(|n| n.to_string() == "c")
            .expect("c in sorted");
        let pos_d = sorted
            .iter()
            .position(|n| n.to_string() == "d")
            .expect("d in sorted");

        // d must come before b and c
        assert!(pos_d < pos_b, "d should come before b");
        assert!(pos_d < pos_c, "d should come before c");

        // b and c must come before a
        assert!(pos_b < pos_a, "b should come before a");
        assert!(pos_c < pos_a, "c should come before a");
    }

    #[test]
    fn test_topo_sort_multiple_roots() {
        // Graph: a -> c
        //        b -> c
        // Expected: c before both a and b
        let graph = build_test_graph(vec![
            ("c".parse().expect("valid package name"), vec![]),
            (
                "a".parse().expect("valid package name"),
                vec!["c".parse().expect("valid package name")],
            ),
            (
                "b".parse().expect("valid package name"),
                vec!["c".parse().expect("valid package name")],
            ),
        ]);

        let sorted = graph.topological_sort().expect("sort should succeed");

        assert_eq!(sorted.len(), 3);

        let pos_a = sorted
            .iter()
            .position(|n| n.to_string() == "a")
            .expect("a in sorted");
        let pos_b = sorted
            .iter()
            .position(|n| n.to_string() == "b")
            .expect("b in sorted");
        let pos_c = sorted
            .iter()
            .position(|n| n.to_string() == "c")
            .expect("c in sorted");

        // c must come before both a and b
        assert!(pos_c < pos_a, "c should come before a");
        assert!(pos_c < pos_b, "c should come before b");
    }

    #[test]
    fn test_topo_sort_complex_graph() {
        // More complex graph:
        //   e <- d <- a
        //   e <- c <-/
        //   b <- c
        // Expected: e before d and c, d and c before a, c before b
        let graph = build_test_graph(vec![
            ("e".parse().expect("valid package name"), vec![]),
            (
                "d".parse().expect("valid package name"),
                vec!["e".parse().expect("valid package name")],
            ),
            (
                "c".parse().expect("valid package name"),
                vec!["e".parse().expect("valid package name")],
            ),
            (
                "b".parse().expect("valid package name"),
                vec!["c".parse().expect("valid package name")],
            ),
            (
                "a".parse().expect("valid package name"),
                vec![
                    "d".parse().expect("valid package name"),
                    "c".parse().expect("valid package name"),
                ],
            ),
        ]);

        let sorted = graph.topological_sort().expect("sort should succeed");

        assert_eq!(sorted.len(), 5);

        let pos_a = sorted
            .iter()
            .position(|n| n.to_string() == "a")
            .expect("a in sorted");
        let pos_b = sorted
            .iter()
            .position(|n| n.to_string() == "b")
            .expect("b in sorted");
        let pos_c = sorted
            .iter()
            .position(|n| n.to_string() == "c")
            .expect("c in sorted");
        let pos_d = sorted
            .iter()
            .position(|n| n.to_string() == "d")
            .expect("d in sorted");
        let pos_e = sorted
            .iter()
            .position(|n| n.to_string() == "e")
            .expect("e in sorted");

        // e must come before d and c
        assert!(pos_e < pos_d, "e should come before d");
        assert!(pos_e < pos_c, "e should come before c");

        // d and c must come before a
        assert!(pos_d < pos_a, "d should come before a");
        assert!(pos_c < pos_a, "c should come before a");

        // c must come before b
        assert!(pos_c < pos_b, "c should come before b");
    }

    #[test]
    fn test_topo_sort_single_node() {
        // Graph with just one node
        let graph = build_test_graph(vec![("a".parse().expect("valid package name"), vec![])]);

        let sorted = graph.topological_sort().expect("sort should succeed");

        assert_eq!(sorted.len(), 1);
        assert_eq!(sorted[0].to_string(), "a");
    }

    #[test]
    fn test_topo_sort_parallel_chains() {
        // Two independent chains: a -> b and c -> d
        let graph = build_test_graph(vec![
            ("b".parse().expect("valid package name"), vec![]),
            (
                "a".parse().expect("valid package name"),
                vec!["b".parse().expect("valid package name")],
            ),
            ("d".parse().expect("valid package name"), vec![]),
            (
                "c".parse().expect("valid package name"),
                vec!["d".parse().expect("valid package name")],
            ),
        ]);

        let sorted = graph.topological_sort().expect("sort should succeed");

        assert_eq!(sorted.len(), 4);

        let pos_a = sorted
            .iter()
            .position(|n| n.to_string() == "a")
            .expect("a in sorted");
        let pos_b = sorted
            .iter()
            .position(|n| n.to_string() == "b")
            .expect("b in sorted");
        let pos_c = sorted
            .iter()
            .position(|n| n.to_string() == "c")
            .expect("c in sorted");
        let pos_d = sorted
            .iter()
            .position(|n| n.to_string() == "d")
            .expect("d in sorted");

        // Within each chain, dependencies come first
        assert!(pos_b < pos_a, "b should come before a");
        assert!(pos_d < pos_c, "d should come before c");
    }

    #[test]
    fn test_topo_sort_detects_cycle() {
        // Manually construct a graph with a cycle: a -> b -> a
        // This tests that topological_sort detects cycles as a safety net
        let mut nodes = HashMap::new();

        nodes.insert(
            "a".parse().expect("valid package name"),
            DependencyNode {
                name: "a".parse().expect("valid package name"),
                package_type: Some(PackageType::Lib),
                source: DependencySource::Local {
                    path: PathBuf::from("/tmp"),
                },
                dependencies: vec!["b".parse().expect("valid package name")],
                version: VersionReq::STAR,
            },
        );

        nodes.insert(
            "b".parse().expect("valid package name"),
            DependencyNode {
                name: "b".parse().expect("valid package name"),
                package_type: Some(PackageType::Lib),
                source: DependencySource::Local {
                    path: PathBuf::from("/tmp"),
                },
                dependencies: vec!["a".parse().expect("valid package name")],
                version: VersionReq::STAR,
            },
        );

        let graph = DependencyGraph { nodes };

        let result = graph.topological_sort();
        assert!(result.is_err(), "should detect cycle");

        let err = result.unwrap_err();
        assert!(
            matches!(err, DependencyError::CircularDependency(_)),
            "error should be CircularDependency"
        );
    }

    #[test]
    fn test_topo_sort_empty_graph() {
        let graph = DependencyGraph {
            nodes: HashMap::new(),
        };

        let sorted = graph.topological_sort().expect("sort should succeed");
        assert_eq!(sorted.len(), 0);
    }
}
