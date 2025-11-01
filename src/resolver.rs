// (c) Copyright 2025 Helsing GmbH. All rights reserved.

use std::{
    collections::{HashMap, HashSet, VecDeque},
    path::{Path, PathBuf},
};

use async_recursion::async_recursion;
use miette::{Context as _, Diagnostic, bail};
use semver::VersionReq;
use thiserror::Error;

use crate::{
    credentials::Credentials,
    manifest::{
        BuffrsManifest, Dependency, DependencyManifest, LocalDependencyManifest, MANIFEST_FILE,
        PackagesManifest,
    },
    package::{PackageName, PackageType},
    registry::RegistryUri,
};

/// Models the source of a dependency
#[derive(Debug, Clone)]
pub enum DependencySource {
    /// A local dependencies, expressed by it's path
    Local {
        /// Local path
        path: PathBuf,
    },
    /// A remote dependencies, expressed by it's repo & registry
    Remote {
        /// Registry
        registry: RegistryUri,
        /// Repository name
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

/// Maps a package name to metadata describing the package
pub type MetadataMap = HashMap<PackageName, DependencyNode>;

/// Represents a resolved dependency with its name and metadata
#[derive(Debug)]
pub struct DependencyDetails {
    /// The package name
    pub name: PackageName,
    /// The dependency node containing metadata
    pub node: DependencyNode,
}

/// A dependency graph containing nodes and edges
#[derive(Debug)]
pub struct DependencyGraph {
    /// Map of package names to their metadata
    pub nodes: MetadataMap,
}

impl DependencyGraph {
    /// Build a dependency graph from a manifest
    ///
    /// Downloads remote packages to discover their transitive dependencies
    pub async fn build(
        manifest: &PackagesManifest,
        base_path: &Path,
        credentials: &Credentials,
    ) -> miette::Result<Self> {
        let mut builder = GraphBuilder::new(base_path.to_path_buf(), credentials);

        // Get the parent package type from the manifest
        let parent_package_type = manifest.package.as_ref().map(|p| p.kind);

        // Add root dependencies
        for dependency in manifest.dependencies.iter().flatten() {
            builder
                .add_dependency(dependency, parent_package_type)
                .await
                .wrap_err_with(|| {
                    format!(
                        "while resolving dependencies of {}",
                        manifest
                            .package
                            .as_ref()
                            .map(|p| p.name.to_string())
                            .unwrap_or_else(|| "root".to_string())
                    )
                })?;
        }

        Ok(Self {
            nodes: builder.nodes,
        })
    }

    /// Returns dependencies in topological order (dependencies before dependents)
    ///
    /// This is a convenience method that combines topological_sort with node lookup
    pub fn ordered_dependencies(&self) -> Result<Vec<DependencyDetails>, DependencyError> {
        let sorted_names = self.topological_sort()?;
        Ok(sorted_names
            .into_iter()
            .filter_map(|name| {
                self.nodes.get(&name).map(|node| DependencyDetails {
                    name: name.clone(),
                    node: node.clone(),
                })
            })
            .collect())
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
                adj_list.entry(dep.clone()).or_default().push(name.clone());
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

    /// Gets the number of packages that depend on a package
    pub fn dependants_count_of(&self, package_name: &PackageName) -> usize {
        self.nodes
            .values()
            .filter(|node| node.dependencies.contains(package_name))
            .count()
    }
}

/// Internal builder for constructing the dependency graph
struct GraphBuilder<'a> {
    nodes: HashMap<PackageName, DependencyNode>,
    base_path: PathBuf,
    /// Track which packages we're currently visiting to detect cycles during construction
    visiting: HashSet<PackageName>,
    credentials: &'a Credentials,
}

impl<'a> GraphBuilder<'a> {
    fn new(base_path: PathBuf, credentials: &'a Credentials) -> Self {
        Self {
            nodes: HashMap::new(),
            base_path,
            visiting: HashSet::new(),
            credentials,
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
            self.validate_compatibility(dependency, existing)
                .wrap_err_with(|| format!("conflicting dependency on {}", package_name))?;
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

        let manifest = BuffrsManifest::require_package_manifest(&manifest_path).await?;
        let package_type = manifest.package.as_ref().map(|p| p.kind);

        Self::ensure_lib_not_depends_on_api(dependency, parent_type, package_type)?;

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
            self.add_dependency(&sub_dep, package_type)
                .await
                .wrap_err_with(|| {
                    format!("while resolving dependencies of {}", dependency.package)
                })?;
            self.base_path = old_base;
        }

        Ok(())
    }

    /// Ensures that a lib package doesn't depend on an api package
    fn ensure_lib_not_depends_on_api(
        dependency: &Dependency,
        parent_type: Option<PackageType>,
        package_type: Option<PackageType>,
    ) -> miette::Result<()> {
        // Validate package type constraint
        if let Some(PackageType::Lib) = parent_type
            && let Some(PackageType::Api) = package_type
        {
            bail!(DependencyError::InvalidPackageTypeDependency {
                parent: PackageName::unchecked("parent"),
                dependency: dependency.package.clone(),
            });
        }

        Ok(())
    }

    async fn add_remote_dependency(
        &mut self,
        dependency: &Dependency,
        remote_manifest: &crate::manifest::RemoteDependencyManifest,
        parent_type: Option<PackageType>,
    ) -> miette::Result<()> {
        let package_name = &dependency.package;
        let registry = &remote_manifest.registry;
        let repository = &remote_manifest.repository;
        let version = &remote_manifest.version;

        // Download the package to read its manifest and discover dependencies
        let registry_client = registry
            .get_registry(self.credentials)
            .wrap_err_with(|| format!("failed to initialize registry {}", registry))?;

        let downloaded_package = registry_client.download(dependency.clone()).await?;

        // Read the package manifest to discover dependencies and package type
        let manifest = downloaded_package.manifest;
        let package_type = manifest.package.as_ref().map(|p| p.kind);

        Self::ensure_lib_not_depends_on_api(dependency, parent_type, package_type)?;

        let sub_dependencies: Vec<PackageName> = manifest.get_dependency_package_names();

        // Add node with discovered metadata
        self.nodes.insert(
            package_name.clone(),
            DependencyNode {
                name: package_name.clone(),
                package_type,
                source: DependencySource::Remote {
                    registry: registry.clone(),
                    repository: repository.clone(),
                },
                dependencies: sub_dependencies.clone(),
                version: version.clone(),
            },
        );

        // Recursively process transitive dependencies
        for sub_dep in manifest.dependencies.unwrap_or_default() {
            self.add_dependency(&sub_dep, package_type)
                .await
                .wrap_err_with(|| format!("while resolving dependencies of {}", package_name))?;
        }

        Ok(())
    }

    fn validate_compatibility(
        &self,
        dependency: &Dependency,
        existing: &DependencyNode,
    ) -> miette::Result<()> {
        // Check for local/remote conflicts
        self.validate_manifest_conflicts(dependency, existing)?;

        // Check for version conflicts on remote dependencies
        self.validate_version_compatibility(dependency, existing)?;

        Ok(())
    }

    /// Validates that version requirements are compatible when the same package is requested multiple times
    fn validate_version_compatibility(
        &self,
        dependency: &Dependency,
        existing: &DependencyNode,
    ) -> miette::Result<()> {
        // Only check version compatibility for remote dependencies
        if let (DependencyManifest::Remote(new_remote), DependencySource::Remote { .. }) =
            (&dependency.manifest, &existing.source)
        {
            // For now, buffrs only supports pinned versions (see TODO #205)
            // We check if the version requirements are equal since they should be exact pins
            // In the future with dynamic version resolution, this would need to check for compatibility
            if new_remote.version != existing.version {
                bail!(DependencyError::VersionConflict {
                    package: dependency.package.clone(),
                    required_version: new_remote.version.clone(),
                    existing_version: existing.version.clone(),
                });
            }
        }

        Ok(())
    }

    /// Checks for conflicting dependencies between local / remote deps in the dependency tree
    fn validate_manifest_conflicts(
        &self,
        dependency: &Dependency,
        existing: &DependencyNode,
    ) -> miette::Result<()> {
        match (&dependency.manifest, &existing.source) {
            (DependencyManifest::Local(_), DependencySource::Remote { .. }) => {
                bail!(DependencyError::LocalRemoteConflict {
                    package: dependency.package.clone(),
                });
            }
            (DependencyManifest::Remote(_), DependencySource::Local { .. }) => {
                bail!(DependencyError::LocalRemoteConflict {
                    package: dependency.package.clone(),
                });
            }
            _ => {}
        }

        Ok(())
    }
}

/// Errors that can occur during dependency resolution
#[derive(Error, Diagnostic, Debug)]
pub enum DependencyError {
    /// A local dependency conflicts with a remote dependency
    #[error("local/remote dependency conflict for package {package}")]
    LocalRemoteConflict {
        /// The package that has the conflict
        package: PackageName,
    },

    /// A lib package cannot depend on an api package
    #[error("package of type lib cannot depend on package of type api: {parent} -> {dependency}")]
    InvalidPackageTypeDependency {
        /// The parent lib package
        parent: PackageName,
        /// The api package being depended on
        dependency: PackageName,
    },

    /// A circular dependency was detected in the dependency graph
    #[error("circular dependency detected: {0}")]
    CircularDependency(String),

    /// Version conflict between multiple dependants
    #[error(
        "version conflict for {package}: requires {required_version} but already resolved to {existing_version}"
    )]
    VersionConflict {
        /// The package with conflicting versions
        package: PackageName,
        /// The version requirement that conflicts
        required_version: VersionReq,
        /// The version already resolved in the graph
        existing_version: VersionReq,
    },

    /// Failed to download a dependency from the registry
    #[error("failed to download dependency {name}@{version} from the registry")]
    DownloadError {
        /// Package name
        name: PackageName,
        /// Version requirement
        version: VersionReq,
    },
}

// tests moves to ./tests/resolver_v2_tests.rs
