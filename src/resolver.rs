// (c) Copyright 2025 Helsing GmbH. All rights reserved.
// thiserror v2 generates unused_assignments in Display impls when free-function
// positional format args reference fields via .field syntax.
#![allow(unused_assignments)]

use std::{
    collections::{HashMap, VecDeque},
    path::{Path, PathBuf},
};

use miette::{Context as _, Diagnostic, bail};
use semver::{Version, VersionReq};
use thiserror::Error;

use crate::{
    cache::Cache,
    credentials::Credentials,
    lock::{FileRequirement, Lockfile},
    manifest::{
        Dependency, DependencyManifest, MANIFEST_FILE, Manifest, PackagesManifest,
        RemoteDependencyManifest,
    },
    operations::install::NetworkMode,
    package::{PackageName, PackageType},
    registry::{Artifactory, RegistryUri},
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
    /// Exact version requirement (`=<resolved>`) for remote deps, `*` for locals
    ///
    /// Consumed by the installer to fetch the package, so it pins the version the
    /// resolver chose rather than the requirement that produced it.
    pub version: VersionReq,
    /// The concrete version that was resolved and downloaded (None for local deps)
    pub resolved_version: Option<Version>,
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
    /// Whether network access was allowed when building this graph
    pub network_mode: NetworkMode,
}

impl DependencyGraph {
    /// Build a dependency graph from a manifest
    ///
    /// Downloads remote packages to discover their transitive dependencies.
    /// If `network_mode` is [`NetworkMode::Offline`], only cached packages are used
    /// and a [`DependencyError::Offline`] is returned when a download would be
    /// needed.
    pub async fn build(
        manifest: &PackagesManifest,
        base_path: &Path,
        credentials: &Credentials,
        lockfile: Option<Lockfile>,
        network_mode: NetworkMode,
    ) -> miette::Result<Self> {
        let root_package_type = manifest.package.as_ref().map(|p| p.kind);
        let root_name = manifest
            .package
            .as_ref()
            .map(|p| p.name.to_string())
            .unwrap_or_else(|| "root".to_string());

        let mut builder = GraphBuilder::new(
            base_path.to_path_buf(),
            root_package_type,
            credentials,
            lockfile,
            network_mode,
        );

        // Phase 1: register every root edge before resolving anything, so a package
        // required by several roots is picked against the merged requirement set.
        for dependency in manifest.dependencies.iter().flatten() {
            builder
                .add_edge(None, dependency)
                .wrap_err_with(|| format!("while resolving dependencies of {}", root_name))?;
        }

        // Phase 2: drain the worklist.
        builder
            .drive()
            .await
            .wrap_err_with(|| format!("while resolving dependencies of {}", root_name))?;

        let graph = Self {
            nodes: builder.nodes,
            network_mode,
        };

        // The worklist is BFS, so a cycle produces a cyclic graph rather than
        // unbounded recursion. Detect it on the finished graph.
        graph.topological_sort()?;

        Ok(graph)
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

/// A single edge contributing a `VersionReq` to a package's accumulated set.
#[derive(Debug, Clone)]
struct RequirementEdge {
    /// The contributing parent package, or `None` for a root manifest edge.
    from: Option<PackageName>,
    /// The version requirement carried by this edge. `VersionReq::STAR` for locals.
    req: VersionReq,
}

/// Internal builder for constructing the dependency graph
struct GraphBuilder<'a> {
    nodes: HashMap<PackageName, DependencyNode>,
    /// Per-package accumulated requirement edges.
    requirements: HashMap<PackageName, Vec<RequirementEdge>>,
    /// First-seen source for each package, used to detect local/remote conflicts.
    sources: HashMap<PackageName, DependencySource>,
    /// FIFO of package names whose resolution must be (re-)attempted.
    worklist: VecDeque<PackageName>,
    base_path: PathBuf,
    /// Package type of the root manifest, used as the parent type for root edges.
    root_package_type: Option<PackageType>,
    credentials: &'a Credentials,
    lockfile: Option<Lockfile>,
    registry_clients: HashMap<RegistryUri, Artifactory>,
    network_mode: NetworkMode,
}

impl<'a> GraphBuilder<'a> {
    fn new(
        base_path: PathBuf,
        root_package_type: Option<PackageType>,
        credentials: &'a Credentials,
        lockfile: Option<Lockfile>,
        network_mode: NetworkMode,
    ) -> Self {
        Self {
            nodes: HashMap::new(),
            requirements: HashMap::new(),
            sources: HashMap::new(),
            worklist: VecDeque::new(),
            base_path,
            root_package_type,
            credentials,
            lockfile,
            registry_clients: HashMap::new(),
            network_mode,
        }
    }

    /// Records a requirement edge from `parent` (None = root manifest) to
    /// `dependency`, scheduling the dependency if it needs (re-)resolution.
    fn add_edge(
        &mut self,
        parent: Option<PackageName>,
        dependency: &Dependency,
    ) -> miette::Result<()> {
        let name = dependency.package.clone();

        let new_source = match &dependency.manifest {
            DependencyManifest::Local(local) => DependencySource::Local {
                path: self.base_path.join(&local.path),
            },
            DependencyManifest::Remote(remote) => DependencySource::Remote {
                registry: remote.registry.clone(),
                repository: remote.repository.clone(),
            },
        };

        match self.sources.get(&name) {
            Some(existing) => Self::ensure_source_kinds_match(&name, existing, &new_source)?,
            None => {
                self.sources.insert(name.clone(), new_source);
            }
        }

        let req = match &dependency.manifest {
            DependencyManifest::Local(_) => VersionReq::STAR,
            DependencyManifest::Remote(remote) => remote.version.clone(),
        };

        self.requirements
            .entry(name.clone())
            .or_default()
            .push(RequirementEdge { from: parent, req });

        if self.needs_visit(&name) && !self.worklist.contains(&name) {
            self.worklist.push_back(name);
        }

        Ok(())
    }

    /// True when `name` has no node yet, or its resolved version no longer
    /// satisfies the (possibly just-extended) requirement set.
    ///
    /// The conditional scheduling this drives is what makes the traversal
    /// terminate on cyclic graphs: a back-edge into an already-inserted node
    /// schedules no further work.
    fn needs_visit(&self, name: &PackageName) -> bool {
        match self.nodes.get(name) {
            None => true,
            Some(node) => match &node.resolved_version {
                // Locals carry no version; once resolved they never need revisiting.
                None => false,
                Some(version) => !self
                    .requirements
                    .get(name)
                    .is_some_and(|edges| edges.iter().all(|e| e.req.matches(version))),
            },
        }
    }

    fn ensure_source_kinds_match(
        name: &PackageName,
        existing: &DependencySource,
        new_kind: &DependencySource,
    ) -> miette::Result<()> {
        let mismatch = matches!(
            (existing, new_kind),
            (
                DependencySource::Local { .. },
                DependencySource::Remote { .. }
            ) | (
                DependencySource::Remote { .. },
                DependencySource::Local { .. }
            )
        );

        if mismatch {
            bail!(DependencyError::LocalRemoteConflict {
                package: name.clone()
            });
        }

        Ok(())
    }

    /// Drains the worklist, resolving each package against its accumulated
    /// requirement set and walking its transitives.
    ///
    /// Terminates because a package's merged requirement set only grows while its
    /// parent set is stable (which can only hold or lower the chosen version),
    /// retraction never raises a pick, and `fetch_and_walk` short-circuits when the
    /// pick is unchanged — so each package is walked at most once per distinct
    /// version, over a finite, non-increasing sequence.
    async fn drive(&mut self) -> miette::Result<()> {
        while let Some(name) = self.worklist.pop_front() {
            // The package may have been retracted out of existence, or satisfied by
            // an earlier pop, since it was scheduled.
            if !self.requirements.contains_key(&name) || !self.needs_visit(&name) {
                continue;
            }

            self.resolve_one(&name)
                .await
                .wrap_err_with(|| format!("while resolving {}", name))?;
        }

        Ok(())
    }

    async fn resolve_one(&mut self, name: &PackageName) -> miette::Result<()> {
        let source = self
            .sources
            .get(name)
            .cloned()
            .expect("source recorded by add_edge");

        match source {
            DependencySource::Local { path } => self.resolve_local(name, &path).await,
            DependencySource::Remote {
                registry,
                repository,
            } => self.resolve_remote(name, &registry, &repository).await,
        }
    }

    async fn resolve_local(
        &mut self,
        name: &PackageName,
        resolved_path: &Path,
    ) -> miette::Result<()> {
        let manifest_path = resolved_path.join(MANIFEST_FILE);
        let manifest = Manifest::require_package_manifest(&manifest_path).await?;
        let package_type = manifest.package.as_ref().map(|p| p.kind);

        self.ensure_lib_not_depends_on_api(name, package_type)?;

        // Insert before registering sub-edges so a back-edge into this package
        // resolves against an existing node instead of looping.
        self.nodes.insert(
            name.clone(),
            DependencyNode {
                name: name.clone(),
                package_type,
                source: DependencySource::Local {
                    path: resolved_path.to_path_buf(),
                },
                dependencies: manifest.get_dependency_package_names(),
                version: VersionReq::STAR,
                resolved_version: None,
            },
        );

        // Sub-dependency paths are relative to this package's directory.
        let old_base = std::mem::replace(&mut self.base_path, resolved_path.to_path_buf());
        let mut result = Ok(());
        for sub_dep in manifest.dependencies.into_iter().flatten() {
            result = self.add_edge(Some(name.clone()), &sub_dep);
            if result.is_err() {
                break;
            }
        }
        self.base_path = old_base;

        result
    }

    async fn resolve_remote(
        &mut self,
        name: &PackageName,
        registry: &RegistryUri,
        repository: &str,
    ) -> miette::Result<()> {
        let reqs: Vec<VersionReq> = self.requirements[name]
            .iter()
            .map(|e| e.req.clone())
            .collect();

        // 1. Lockfile: the highest pinned version satisfying every requirement.
        //    A pin that satisfies nothing is skipped, not an error — in a workspace
        //    pool it may belong to another, independently-resolved member.
        if let Some(lockfile) = &self.lockfile
            && let Some((locked_version, file_req)) = lockfile.find_satisfying_all(name, &reqs)
            && file_req.url().as_str().starts_with(registry.as_str())
        {
            tracing::debug!("lockfile pins {}@{}", name, locked_version);

            return self
                .fetch_and_walk(name, registry, repository, locked_version, Some(file_req))
                .await;
        }

        // 2. Registry.
        let artifactory = self.registry_client(registry)?;
        let (chosen, available) = artifactory
            .resolve_version_for_all(repository.to_string(), name.clone(), &reqs)
            .await
            .wrap_err_with(|| format!("could not resolve {} from registry {}", name, registry))?;

        let chosen = chosen.ok_or_else(|| DependencyError::NoCompatibleVersion {
            package: name.clone(),
            requirements: reqs,
            available_versions: available,
        })?;

        tracing::debug!("resolved {} -> {}", name, chosen);

        self.fetch_and_walk(name, registry, repository, chosen, None)
            .await
    }

    async fn fetch_and_walk(
        &mut self,
        name: &PackageName,
        registry: &RegistryUri,
        repository: &str,
        chosen: Version,
        cached: Option<FileRequirement>,
    ) -> miette::Result<()> {
        if let Some(existing) = self.nodes.get(name) {
            if existing.resolved_version.as_ref() == Some(&chosen) {
                return Ok(());
            }

            // The pick changed: everything the old version pulled in is no longer
            // justified by this package.
            self.retract_contributions_from(name);
        }

        let cache = Cache::open().await?;
        let cached_pkg = match cached {
            Some(file_req) => cache.get(file_req).await.ok().flatten(),
            None => None,
        };

        let package = match (cached_pkg, self.network_mode) {
            (Some(pkg), _) => {
                tracing::debug!("resolved {}@{} from local cache", name, chosen);
                pkg
            }
            (None, NetworkMode::Online) => {
                tracing::debug!("downloading {}@{} from registry", name, chosen);

                let artifactory = self.registry_client(registry)?;
                let dependency = Dependency {
                    package: name.clone(),
                    manifest: DependencyManifest::Remote(RemoteDependencyManifest {
                        registry: registry.clone(),
                        repository: repository.to_string(),
                        version: exact_req(&chosen),
                    }),
                };

                artifactory.download(dependency, &chosen).await?
            }
            (None, NetworkMode::Offline) => {
                bail!(DependencyError::Offline {
                    name: name.clone(),
                    requirements: self.requirements[name]
                        .iter()
                        .map(|e| e.req.clone())
                        .collect(),
                });
            }
        };

        let manifest = package.manifest;
        let package_type = manifest.package.as_ref().map(|p| p.kind);

        self.ensure_lib_not_depends_on_api(name, package_type)?;

        // `version` is consumed by the installer to fetch the package, so it must
        // pin the version the resolver actually chose — not one of the requirements
        // that produced it, which would let the installer re-resolve upward.
        self.nodes.insert(
            name.clone(),
            DependencyNode {
                name: name.clone(),
                package_type,
                source: DependencySource::Remote {
                    registry: registry.clone(),
                    repository: repository.to_string(),
                },
                dependencies: manifest.get_dependency_package_names(),
                version: exact_req(&chosen),
                resolved_version: Some(chosen),
            },
        );

        for sub_dep in manifest.dependencies.into_iter().flatten() {
            self.add_edge(Some(name.clone()), &sub_dep)?;
        }

        Ok(())
    }

    /// Retracts every requirement edge contributed by `parent`, dropping packages
    /// that become unreachable and rescheduling those whose pick no longer fits.
    fn retract_contributions_from(&mut self, parent: &PackageName) {
        let mut frontier: Vec<PackageName> = vec![parent.clone()];

        while let Some(retractor) = frontier.pop() {
            let affected: Vec<PackageName> = self
                .requirements
                .iter()
                .filter(|(_, edges)| edges.iter().any(|e| e.from.as_ref() == Some(&retractor)))
                .map(|(name, _)| name.clone())
                .collect();

            for name in affected {
                // Never retract the originating package's own requirement set.
                if name == *parent {
                    continue;
                }

                let Some(edges) = self.requirements.get_mut(&name) else {
                    continue;
                };
                edges.retain(|e| e.from.as_ref() != Some(&retractor));

                if edges.is_empty() {
                    self.requirements.remove(&name);
                    self.sources.remove(&name);
                    self.nodes.remove(&name);
                    frontier.push(name);
                } else if self.needs_visit(&name) && !self.worklist.contains(&name) {
                    // Still reachable, but the shrunken set no longer fits the pick.
                    // We deliberately do not re-optimize upward.
                    self.worklist.push_back(name);
                }
            }
        }
    }

    /// Ensures that no lib package depends on an api package
    ///
    /// Every `edge.from` is guaranteed to be in `nodes` already, because a parent
    /// inserts its own node before registering sub-edges. Root edges (`from: None`)
    /// use the root manifest's package type.
    fn ensure_lib_not_depends_on_api(
        &self,
        name: &PackageName,
        package_type: Option<PackageType>,
    ) -> miette::Result<()> {
        if package_type != Some(PackageType::Api) {
            return Ok(());
        }

        let Some(edges) = self.requirements.get(name) else {
            return Ok(());
        };

        for edge in edges {
            let parent_type = match &edge.from {
                None => self.root_package_type,
                Some(parent) => self.nodes.get(parent).and_then(|n| n.package_type),
            };

            if parent_type == Some(PackageType::Lib) {
                bail!(DependencyError::InvalidPackageTypeDependency {
                    parent: edge
                        .from
                        .clone()
                        .unwrap_or_else(|| PackageName::unchecked("parent")),
                    dependency: name.clone(),
                });
            }
        }

        Ok(())
    }

    fn registry_client(&mut self, registry: &RegistryUri) -> miette::Result<Artifactory> {
        if let Some(client) = self.registry_clients.get(registry) {
            return Ok(client.clone());
        }

        let client = Artifactory::new(registry.clone(), self.credentials)
            .wrap_err_with(|| format!("failed to initialize registry {}", registry))?;
        self.registry_clients
            .insert(registry.clone(), client.clone());

        Ok(client)
    }
}

/// Builds an exact (`=x.y.z`) requirement from a resolved version.
fn exact_req(version: &Version) -> VersionReq {
    VersionReq::parse(&format!("={}", version)).expect("resolved version is a valid exact req")
}

fn fmt_reqs(reqs: &[semver::VersionReq]) -> String {
    reqs.iter()
        .map(|r| r.to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

fn fmt_versions(versions: &[semver::Version]) -> String {
    if versions.is_empty() {
        "(none published)".to_string()
    } else {
        versions
            .iter()
            .map(|v| v.to_string())
            .collect::<Vec<_>>()
            .join(", ")
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

    /// A network request was needed but --offline mode is active
    #[error("cannot fetch {name} in offline mode (required: [{}])", fmt_reqs(.requirements))]
    #[diagnostic(help(
        "run `buffrs install` without --offline to cache the package,\n\
         or set BUFFRS_CACHE to a pre-populated cache directory"
    ))]
    Offline {
        /// Package name
        name: PackageName,
        /// Version requirements that could not be satisfied offline
        requirements: Vec<semver::VersionReq>,
    },

    /// No single version satisfies the union of all requirements.
    #[error(
        "no version of {package} satisfies all requirements: [{}]; available versions: [{}]",
        fmt_reqs(.requirements),
        fmt_versions(.available_versions)
    )]
    NoCompatibleVersion {
        /// The package with conflicting requirements
        package: PackageName,
        /// The set of requirements that could not be jointly satisfied
        requirements: Vec<semver::VersionReq>,
        /// All versions that were published for this package
        available_versions: Vec<semver::Version>,
    },
}

#[cfg(test)]
mod error_tests {
    use super::*;
    use semver::Version;

    #[test]
    fn no_compatible_version_diagnostic_lists_requirements() {
        let err = DependencyError::NoCompatibleVersion {
            package: "leaf-lib".parse().unwrap(),
            requirements: vec!["^1.1.0".parse().unwrap(), "<=1.2.0".parse().unwrap()],
            available_versions: vec![Version::new(1, 0, 0), Version::new(1, 5, 0)],
        };
        let msg = err.to_string();
        assert!(msg.contains("leaf-lib"), "msg: {msg}");
        assert!(msg.contains("1.0.0"), "msg: {msg}");
        assert!(msg.contains("1.5.0"), "msg: {msg}");
        assert!(msg.contains("^1.1.0"), "msg: {msg}");
        assert!(msg.contains("<=1.2.0"), "msg: {msg}");
    }
}

#[cfg(test)]
mod retraction_tests {
    use super::*;

    fn pkg(name: &str) -> PackageName {
        PackageName::unchecked(name)
    }

    fn remote_source() -> DependencySource {
        DependencySource::Remote {
            registry: "http://example.test/artifactory".parse().unwrap(),
            repository: "test-repo".to_string(),
        }
    }

    fn node(name: &str, version: &str, dependencies: Vec<PackageName>) -> DependencyNode {
        let resolved: Version = version.parse().unwrap();

        DependencyNode {
            name: pkg(name),
            package_type: Some(PackageType::Lib),
            source: remote_source(),
            dependencies,
            version: exact_req(&resolved),
            resolved_version: Some(resolved),
        }
    }

    /// Builds a graph state directly, bypassing any network access.
    fn builder<'a>(
        credentials: &'a Credentials,
        edges: &[(&str, Option<&str>, &str)],
        nodes: Vec<DependencyNode>,
    ) -> GraphBuilder<'a> {
        let mut builder = GraphBuilder::new(
            PathBuf::from("/nonexistent"),
            Some(PackageType::Lib),
            credentials,
            None,
            NetworkMode::Offline,
        );

        for (name, from, req) in edges {
            builder
                .requirements
                .entry(pkg(name))
                .or_default()
                .push(RequirementEdge {
                    from: from.map(pkg),
                    req: req.parse().unwrap(),
                });
            builder
                .sources
                .entry(pkg(name))
                .or_insert_with(remote_source);
        }

        for node in nodes {
            builder.nodes.insert(node.name.clone(), node);
        }

        builder
    }

    /// Dropping a package must transitively drop everything that was only
    /// reachable through it, not just its immediate dependencies.
    ///
    ///   root -> a          (retracted)
    ///   a    -> b          (b's only contributor)
    ///   b    -> c          (c's only contributor)
    #[test]
    fn retraction_cascades_through_multiple_levels() {
        let credentials = Credentials::default();
        let mut builder = builder(
            &credentials,
            &[
                ("a", None, "^1.0.0"),
                ("b", Some("a"), "^1.0.0"),
                ("c", Some("b"), "^1.0.0"),
            ],
            vec![
                node("a", "1.5.0", vec![pkg("b")]),
                node("b", "1.0.0", vec![pkg("c")]),
                node("c", "1.0.0", vec![]),
            ],
        );

        builder.retract_contributions_from(&pkg("a"));

        assert!(
            !builder.nodes.contains_key(&pkg("b")),
            "b was only reachable via a and must be dropped"
        );
        assert!(
            !builder.nodes.contains_key(&pkg("c")),
            "c was only reachable via b and must cascade out too"
        );
        for name in ["b", "c"] {
            assert!(!builder.requirements.contains_key(&pkg(name)));
            assert!(!builder.sources.contains_key(&pkg(name)));
        }

        assert!(
            builder.nodes.contains_key(&pkg("a")),
            "the retracting package itself is left in place for its caller to replace"
        );
        assert!(
            builder.requirements.contains_key(&pkg("a")),
            "a's own requirement set must survive retraction"
        );
    }

    /// A package that keeps a contributor survives, but must be rescheduled when
    /// the shrunken requirement set no longer admits its current version.
    #[test]
    fn retraction_reschedules_survivor_whose_pick_no_longer_fits() {
        let credentials = Credentials::default();
        let mut builder = builder(
            &credentials,
            &[
                ("a", None, "^1.0.0"),
                ("other", None, "^1.0.0"),
                // `shared` is required by both `a` (loosely) and `other` (tightly).
                ("shared", Some("a"), "^1.0.0"),
                ("shared", Some("other"), "<=1.2.0"),
            ],
            vec![
                node("a", "1.5.0", vec![pkg("shared")]),
                node("other", "1.0.0", vec![pkg("shared")]),
                // Currently at 1.5.0, which only `a`'s requirement admits.
                node("shared", "1.5.0", vec![]),
            ],
        );

        builder.retract_contributions_from(&pkg("a"));

        assert!(
            builder.nodes.contains_key(&pkg("shared")),
            "shared is still reachable via other and must not be dropped"
        );
        assert_eq!(
            builder.requirements[&pkg("shared")].len(),
            1,
            "only a's edge is retracted"
        );
        assert!(
            builder.worklist.contains(&pkg("shared")),
            "1.5.0 no longer satisfies <=1.2.0, so shared must be re-resolved"
        );
    }

    /// A survivor whose current version still satisfies the shrunken set is left
    /// alone: retraction never re-optimizes upward.
    #[test]
    fn retraction_leaves_still_satisfied_survivor_alone() {
        let credentials = Credentials::default();
        let mut builder = builder(
            &credentials,
            &[
                ("a", None, "^1.0.0"),
                ("other", None, "^1.0.0"),
                ("shared", Some("a"), "^1.0.0"),
                ("shared", Some("other"), ">=1.0.0"),
            ],
            vec![
                node("a", "1.5.0", vec![pkg("shared")]),
                node("other", "1.0.0", vec![pkg("shared")]),
                node("shared", "1.5.0", vec![]),
            ],
        );

        builder.retract_contributions_from(&pkg("a"));

        assert!(builder.nodes.contains_key(&pkg("shared")));
        assert!(
            !builder.worklist.contains(&pkg("shared")),
            "1.5.0 still satisfies >=1.0.0, so no re-resolution is scheduled"
        );
        assert_eq!(
            builder.nodes[&pkg("shared")].resolved_version,
            Some(Version::new(1, 5, 0)),
            "the existing pick is kept rather than re-optimized"
        );
    }
}
