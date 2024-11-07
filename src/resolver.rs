use std::{collections::HashMap, path::PathBuf, sync::Arc};

use async_recursion::async_recursion;
use miette::{bail, ensure, Context, Diagnostic};
use semver::VersionReq;
use thiserror::Error;

use crate::{
    cache::{Cache, Entry},
    credentials::Credentials,
    lock::{FileRequirement, Lockfile},
    manifest::{
        Dependency, DependencyManifest, LocalDependencyManifest, Manifest,
        RemoteDependencyManifest, MANIFEST_FILE,
    },
    package::{Package, PackageName, PackageStore},
    registry::{Artifactory, RegistryUri},
};

/// Represents a dependency contextualized by the current dependency graph
pub enum ResolvedDependency {
    /// A resolved dependency that is located on a remote registry
    Remote {
        /// The materialized package as downloaded from the registry
        package: Package,
        /// The registry the package was downloaded from
        registry: RegistryUri,
        /// The repository in the registry where the package can be found
        repository: String,
        /// Packages that requested this dependency (and what versions they accept)
        dependants: Vec<Dependant>,
        /// Transitive dependencies
        depends_on: Vec<PackageName>,
    },
    /// A resolved depnedency that is located on the filesystem
    Local {
        /// The materialized package that was created from the buffrs package at the given path
        package: Package,
        /// Location of the requested package
        path: PathBuf,
        /// Packages that requested this dependency (and what versions they accept)
        dependants: Vec<Dependant>,
        /// Transitive dependencies
        depends_on: Vec<PackageName>,
    },
}

impl ResolvedDependency {
    pub(crate) fn package(&self) -> &Package {
        match self {
            Self::Remote { package, .. } => package,
            Self::Local { package, .. } => package,
        }
    }

    pub(crate) fn depends_on(&self) -> &[PackageName] {
        match self {
            Self::Remote { depends_on, .. } => depends_on,
            Self::Local { depends_on, .. } => depends_on,
        }
    }
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

#[derive(Debug, Clone, Eq, PartialEq)]
struct RemoteDependency {
    package: PackageName,
    manifest: RemoteDependencyManifest,
}

impl From<RemoteDependency> for Dependency {
    fn from(value: RemoteDependency) -> Self {
        Dependency {
            package: value.package,
            manifest: value.manifest.into(),
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct LocalDependency {
    package: PackageName,
    manifest: LocalDependencyManifest,
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

    async fn process_dependency(
        name: PackageName,
        dependency: Dependency,
        is_root: bool,
        lockfile: &Lockfile,
        credentials: &Arc<Credentials>,
        cache: &Cache,
        entries: &mut HashMap<PackageName, ResolvedDependency>,
    ) -> miette::Result<()> {
        match dependency.manifest {
            DependencyManifest::Remote(manifest) => {
                Self::process_remote_dependency(
                    name.clone(),
                    RemoteDependency {
                        package: dependency.package,
                        manifest,
                    },
                    is_root,
                    lockfile,
                    credentials,
                    cache,
                    entries,
                )
                .await?;
            }
            DependencyManifest::Local(manifest) => {
                Self::process_local_dependency(
                    name.clone(),
                    LocalDependency {
                        package: dependency.package,
                        manifest,
                    },
                    is_root,
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

    #[async_recursion]
    async fn process_local_dependency(
        name: PackageName,
        dependency: LocalDependency,
        _: bool,
        lockfile: &Lockfile,
        credentials: &Arc<Credentials>,
        cache: &Cache,
        entries: &mut HashMap<PackageName, ResolvedDependency>,
    ) -> miette::Result<()> {
        let manifest = Manifest::try_read_from(&dependency.manifest.path.join(MANIFEST_FILE))
            .await?
            .ok_or_else(|| {
                miette::miette!(
                    "no `{}` for package {} found at path {}",
                    MANIFEST_FILE,
                    dependency.package,
                    dependency.manifest.path.join(MANIFEST_FILE).display()
                )
            })?;

        let store = PackageStore::open(&dependency.manifest.path).await?;
        let package = store.release(&manifest).await?;

        let dependency_name = package.name().clone();
        let sub_dependencies = package.manifest.dependencies.clone();
        let sub_dependency_names: Vec<_> = sub_dependencies
            .iter()
            .map(|sub_dependency| sub_dependency.package.clone())
            .collect();

        entries.insert(
            dependency_name.clone(),
            ResolvedDependency::Local {
                package,
                path: dependency.manifest.path,
                dependants: vec![Dependant {
                    name,
                    version_req: VersionReq::STAR,
                }],
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

        Ok(())
    }

    #[async_recursion]
    async fn process_remote_dependency(
        name: PackageName,
        dependency: RemoteDependency,
        is_root: bool,
        lockfile: &Lockfile,
        credentials: &Arc<Credentials>,
        cache: &Cache,
        entries: &mut HashMap<PackageName, ResolvedDependency>,
    ) -> miette::Result<()> {
        let version_req = dependency.manifest.version.clone();

        if let Some(entry) = entries.get_mut(&dependency.package) {
            match entry {
                ResolvedDependency::Local {
                    path, dependants, ..
                } => {
                    bail!(
                        "a dependency of your project requires {}@{} which collides with a local dependency for {}@{} required by {:?}", 
                        dependency.package,
                        dependency.manifest.version,
                        dependency.package,
                        path.display(),
                        dependants[0].name.clone(),
                    );
                }
                ResolvedDependency::Remote {
                    package,
                    dependants,
                    ..
                } => {
                    ensure!(
                        version_req.matches(package.version()),
                        "a dependency of your project requires {}@{} which collides with {}@{} required by {:?}",
                        dependency.package,
                        dependency.manifest.version,
                        package.name(),
                        package.version(),
                        dependants[0].name.clone(),
                    );

                    dependants.push(Dependant { name, version_req });
                }
            }
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
                ResolvedDependency::Remote {
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
        dependency: RemoteDependency,
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

            let registry = Artifactory::new(&dependency.manifest.registry, credentials).wrap_err(
                DownloadError {
                    name: dependency.package.clone(),
                    version: dependency.manifest.version.clone(),
                },
            )?;

            let package = registry
                // TODO(#205): This works now because buffrs only supports pinned versions.
                // This logic has to change once we implement dynamic version resolution.
                .download(dependency.clone().into())
                .await
                .wrap_err(DownloadError {
                    name: dependency.package,
                    version: dependency.manifest.version,
                })?;

            let file_requirement = FileRequirement::from(local_locked);
            cache
                .put(file_requirement.into(), package.tgz.clone())
                .await
                .ok();

            Ok(package)
        } else {
            let registry = Artifactory::new(&dependency.manifest.registry, credentials).wrap_err(
                DownloadError {
                    name: dependency.package.clone(),
                    version: dependency.manifest.version.clone(),
                },
            )?;

            let package = registry
                .download(dependency.clone().into())
                .await
                .wrap_err(DownloadError {
                    name: dependency.package,
                    version: dependency.manifest.version,
                })?;

            let key = Entry::from(&package);
            let content = package.tgz.clone();
            cache.put(key, content).await.ok();

            Ok(package)
        }
    }

    /// Locates and returns a reference to a resolved dependency package by its name
    pub fn get(&self, name: &PackageName) -> Option<&ResolvedDependency> {
        self.entries.get(name)
    }

    /// Returns a list of all package names in the dependency graph
    pub fn get_package_names(&self) -> Vec<PackageName> {
        self.entries.keys().cloned().collect()
    }
}

impl IntoIterator for DependencyGraph {
    type Item = ResolvedDependency;
    type IntoIter = std::collections::hash_map::IntoValues<PackageName, ResolvedDependency>;

    fn into_iter(self) -> Self::IntoIter {
        self.entries.into_values()
    }
}
