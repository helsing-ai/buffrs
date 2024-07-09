use std::{collections::HashMap, sync::Arc};

use async_recursion::async_recursion;
use miette::{ensure, Context, Diagnostic};
use semver::VersionReq;
use thiserror::Error;

use crate::{
    cache::Cache,
    credentials::Credentials,
    lock::Lockfile,
    manifest::{Dependency, Manifest},
    package::{Package, PackageName},
    registry::{Artifactory, RegistryUri},
};

/// Represents a dependency contextualized by the current dependency graph
pub struct ResolvedDependency {
    /// The materialized package as downloaded from the registry
    pub package: Package,
    /// The registry the package was downloaded from
    pub registry: RegistryUri,
    /// The repository in the registry where the package can be found
    pub repository: String,
    /// Packages that requested this dependency (and what versions they accept)
    pub dependants: Vec<Dependant>,
    /// Transitive dependencies
    pub depends_on: Vec<PackageName>,
}

/// Represents a requester of the associated dependency
pub struct Dependant {
    /// Package that requested the dependency
    pub name: PackageName,
    /// Version requirement
    pub version_req: VersionReq,
}

/// Represents direct and transitive dependencies of the root package
pub struct DependencyGraph {
    entries: HashMap<PackageName, ResolvedDependency>,
}

#[derive(Error, Diagnostic, Debug)]
#[error("failed to download dependency {name}@{version} from the registry")]
struct DownloadError {
    name: PackageName,
    version: VersionReq,
}

impl DependencyGraph {
    /// Recursively resolves dependencies from the manifest to build a dependency graph
    pub async fn from_manifest(
        manifest: &Manifest,
        lockfile: &Lockfile,
        credentials: &Arc<Credentials>,
        cache: &Cache,
    ) -> miette::Result<Self> {
        let name = manifest
            .package
            .as_ref()
            .map(|p| p.name.clone())
            .unwrap_or_else(|| PackageName::unchecked("."));

        let mut entries = HashMap::new();

        for dependency in &manifest.dependencies {
            Self::process_dependency(
                name.clone(),
                dependency.clone(),
                true,
                lockfile,
                credentials,
                cache,
                &mut entries,
            )
            .await?;
        }

        Ok(Self { entries })
    }

    #[async_recursion]
    async fn process_dependency(
        name: PackageName,
        dependency: Dependency,
        is_root: bool,
        lockfile: &Lockfile,
        credentials: &Arc<Credentials>,
        cache: &Cache,
        entries: &mut HashMap<PackageName, ResolvedDependency>,
    ) -> miette::Result<()> {
        let version_req = dependency.manifest.version.clone();
        if let Some(entry) = entries.get_mut(&dependency.package) {
            ensure!(
                version_req.matches(entry.package.version()),
                "a dependency of your project requires {}@{} which collides with {}@{} required by {:?}", 
                dependency.package,
                dependency.manifest.version,
                entry.dependants[0].name.clone(),
                dependency.manifest.version,
                entry.package.manifest.package.as_ref().map(|p| &p.version)
            );

            entry.dependants.push(Dependant { name, version_req });
        } else {
            let dependency_pkg =
                Self::resolve(dependency.clone(), is_root, lockfile, credentials, cache).await?;

            let dependency_name = dependency_pkg.name().clone();
            let sub_dependencies = dependency_pkg.manifest.dependencies.clone();
            let sub_dependency_names: Vec<_> = sub_dependencies
                .iter()
                .map(|sub_dependency| sub_dependency.package.clone())
                .collect();

            entries.insert(
                dependency_name.clone(),
                ResolvedDependency {
                    package: dependency_pkg,
                    registry: dependency.manifest.registry,
                    repository: dependency.manifest.repository,
                    dependants: vec![Dependant { name, version_req }],
                    depends_on: sub_dependency_names,
                },
            );

            for sub_dependency in sub_dependencies {
                Self::process_dependency(
                    dependency_name.clone(),
                    sub_dependency,
                    false,
                    lockfile,
                    credentials,
                    cache,
                    entries,
                )
                .await?;
            }
        }

        Ok(())
    }

    async fn resolve(
        dependency: Dependency,
        is_root: bool,
        lockfile: &Lockfile,
        credentials: &Arc<Credentials>,
        cache: &Cache,
    ) -> miette::Result<Package> {
        if let Some(local_locked) = lockfile.get(&dependency.package) {
            ensure!(
                is_root || dependency.manifest.registry == local_locked.registry,
                "mismatched registry detected for dependency {} - requested {} but lockfile requires {}",
                    dependency.package,
                    dependency.manifest.registry,
                    local_locked.registry,
            );

            // For now we should only check cache if locked package matches manifest,
            // but theoretically we should be able to still look into cache when freshly installing
            // a dependency.
            if dependency.manifest.version.matches(&local_locked.version) {
                if let Some(cached) = cache.get(local_locked.into()).await? {
                    local_locked.validate(&cached)?;
                    return Ok(cached);
                }
            }

            let registry = Artifactory::new(dependency.manifest.registry.clone(), credentials)
                .wrap_err(DownloadError {
                    name: dependency.package.clone(),
                    version: dependency.manifest.version.clone(),
                })?;

            let package = registry
                // TODO(#205): This works now because buffrs only supports pinned versions.
                // This logic has to change once we implement dynamic version resolution.
                .download(dependency.clone())
                .await
                .wrap_err(DownloadError {
                    name: dependency.package,
                    version: dependency.manifest.version,
                })?;

            cache.put(local_locked, package.tgz.clone()).await.ok();

            Ok(package)
        } else {
            let registry = Artifactory::new(dependency.manifest.registry.clone(), credentials)
                .wrap_err(DownloadError {
                    name: dependency.package.clone(),
                    version: dependency.manifest.version.clone(),
                })?;

            registry
                .download(dependency.clone())
                .await
                .wrap_err(DownloadError {
                    name: dependency.package,
                    version: dependency.manifest.version,
                })
        }
    }

    /// Locates and returns a reference to a resolved dependency package by its name
    pub fn get(&self, name: &PackageName) -> Option<&ResolvedDependency> {
        self.entries.get(name)
    }
}

impl IntoIterator for DependencyGraph {
    type Item = ResolvedDependency;
    type IntoIter = std::collections::hash_map::IntoValues<PackageName, ResolvedDependency>;

    fn into_iter(self) -> Self::IntoIter {
        self.entries.into_values()
    }
}
