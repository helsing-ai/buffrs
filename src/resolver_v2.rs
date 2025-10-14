use crate::manifest::{Dependency, DependencyManifest, LocalDependencyManifest, Manifest, MANIFEST_FILE};
use crate::package::{PackageName, PackageType};
use crate::registry::RegistryUri;
use async_recursion::async_recursion;
use miette::{bail, Diagnostic};
use semver::VersionReq;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use thiserror::Error;

/// Models the source of a dependency
#[derive(Debug, Clone)]
pub enum DependencySource {
    /// A local dependencies, expressed by it's path
    Local {
        path: PathBuf,
    },
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

