use std::{
    collections::HashMap,
    env,
    ops::{Deref, DerefMut},
    path::{Path, PathBuf},
};

use async_recursion::async_recursion;
use miette::{bail, ensure, Context, Diagnostic, IntoDiagnostic};
use semver::VersionReq;
use thiserror::Error;

use crate::{
    cache::{Cache, Entry},
    config::Config,
    credentials::Credentials,
    lock::{FileRequirement, Lockfile},
    manifest::{
        Dependency, DependencyManifest, LocalDependencyManifest, Manifest,
        RemoteDependencyManifest, MANIFEST_FILE,
    },
    package::{Package, PackageName, PackageStore},
    registry::{Artifactory, CertValidationPolicy, RegistryRef},
};

/// Represents a dependency contextualized by the current dependency graph
#[derive(Debug, Clone)]
pub enum ResolvedDependency {
    /// A resolved dependency that is located on a remote registry
    Remote {
        /// The materialized package as downloaded from the registry
        package: Package,
        /// The registry the package was downloaded from
        registry: RegistryRef,
        /// The repository in the registry where the package can be found
        repository: String,
        /// Packages that requested this dependency (and what versions they accept)
        dependants: Vec<Dependant>,
        /// Transitive dependencies
        depends_on: Vec<PackageName>,
    },
    /// A resolved dependency that is located on the filesystem
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
#[derive(Debug, Clone)]
pub struct Dependant {
    /// Package that requested the dependency
    pub name: PackageName,
    /// Version requirement
    pub version_req: VersionReq,
}

/// Represents direct and transitive dependencies of the root package
#[derive(Debug, Clone, Default)]
pub struct DependencyGraph {
    pub(crate) entries: HashMap<PackageName, ResolvedDependency>,
}

/// A builder for constructing a dependency graph
pub struct DependencyGraphBuilder<'a> {
    manifest: &'a Manifest,
    lockfile: &'a Lockfile,
    credentials: &'a Credentials,
    cache: &'a Cache,
    preserve_mtime: bool,
    config: &'a Config,
    policy: CertValidationPolicy,
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

impl DependencyGraph {
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

impl Deref for DependencyGraph {
    type Target = HashMap<PackageName, ResolvedDependency>;

    fn deref(&self) -> &Self::Target {
        &self.entries
    }
}

impl DerefMut for DependencyGraph {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.entries
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

impl<'a> DependencyGraphBuilder<'a> {
    /// Creates a new dependency graph builder
    ///
    /// # Parameters
    /// - `manifest`: Manifest of the root package
    /// - `lockfile`: Lockfile of the root package
    /// - `credentials`: Credentials used to authenticate with remote registries
    /// - `cache`: Cache used to store downloaded packages
    /// - `preserve_mtime`: Whether to preserve modification times during package release
    /// - `config`: Configuration settings
    /// - `policy`: Policy used to validate certificates
    ///
    /// # Returns
    /// A new dependency graph builder
    pub fn new(
        manifest: &'a Manifest,
        lockfile: &'a Lockfile,
        credentials: &'a Credentials,
        cache: &'a Cache,
        preserve_mtime: bool,
        config: &'a Config,
        policy: CertValidationPolicy,
    ) -> Self {
        Self {
            manifest,
            lockfile,
            credentials,
            cache,
            config,
            policy,
            preserve_mtime,
        }
    }

    /// Builds the dependency graph
    pub async fn build(self) -> miette::Result<DependencyGraph> {
        let name = self
            .manifest
            .package
            .as_ref()
            .map(|p| p.name.clone())
            .unwrap_or_else(|| PackageName::unchecked("."));

        let parent_dir = env::current_dir().into_diagnostic()?;

        // Prepare the dependency graph
        let mut deps = DependencyGraph::default();

        for dependency in &self.manifest.dependencies {
            self.process_dependency(
                name.clone(),
                dependency.clone(),
                true, // is_root
                &parent_dir,
                &mut deps,
            )
            .await?;
        }

        Ok(deps)
    }

    async fn process_dependency(
        &self,
        name: PackageName,
        dependency: Dependency,
        is_root: bool,
        parent_dir: &Path,
        deps: &mut DependencyGraph,
    ) -> miette::Result<()> {
        match dependency.manifest {
            DependencyManifest::Remote(manifest) => {
                self.process_remote_dependency(
                    name.clone(),
                    RemoteDependency {
                        package: dependency.package,
                        manifest,
                    },
                    is_root,
                    parent_dir,
                    deps,
                )
                .await?;
            }
            DependencyManifest::Local(manifest) => {
                self.process_local_dependency(
                    name.clone(),
                    LocalDependency {
                        package: dependency.package,
                        manifest,
                    },
                    is_root,
                    parent_dir,
                    deps,
                )
                .await?;
            }
        }

        Ok(())
    }

    #[async_recursion]
    async fn process_local_dependency(
        &self,
        name: PackageName,
        dependency: LocalDependency,
        is_root: bool,
        parent_dir: &Path,
        deps: &mut DependencyGraph,
    ) -> miette::Result<()> {
        // If the dependency.manifest_path is relative, it's relative to the parent manifest.
        // We therefore need to resolve it to an absolute path.
        let abs_manifest_dir = if dependency.manifest.path.is_relative() {
            // combine the parent manifest path with the relative path
            parent_dir
                .join(&dependency.manifest.path)
                .canonicalize()
                .into_diagnostic()
                .wrap_err(miette::miette!(
                    "no `{}` for package {} found at path {} referenced by {} as \"{}\"",
                    MANIFEST_FILE,
                    dependency.package,
                    parent_dir.join(&dependency.manifest.path).display(),
                    name,
                    dependency.manifest.path.display()
                ))?
        } else {
            dependency.manifest.path.clone()
        };

        let manifest =
            Manifest::try_read_from(&abs_manifest_dir.join(MANIFEST_FILE), Some(self.config))
                .await?
                .ok_or_else(|| {
                    miette::miette!(
                        "no `{}` for package {} found at path {} referenced by {} as \"{}\"",
                        MANIFEST_FILE,
                        dependency.package,
                        abs_manifest_dir.join(MANIFEST_FILE).display(),
                        name,
                        dependency.manifest.path.display()
                    )
                })?;

        // Process sub-dependencies first
        for sub_dependency in &manifest.dependencies {
            self.process_dependency(
                dependency.package.clone(),
                sub_dependency.clone(),
                true, // is_root
                &abs_manifest_dir,
                deps,
            )
            .await?;
        }

        let package = if is_root {
            let store = PackageStore::open(&abs_manifest_dir).await?;
            let package = store
                .release(&manifest, self.preserve_mtime, self.config, Some(deps))
                .await?;

            // Ensure that the package version doesn't clash with an existing entry,
            // and that it matches the version requirement in the manifest
            if let Some(version_req) = dependency.manifest.publish.map(|p| p.version) {
                let found_version = package.version();

                if let Some(entry) = deps.get_mut(package.name()) {
                    let existing_package = entry.package();
                    ensure!(
                        version_req.matches(existing_package.version()),
                        "a dependency of your project requires {}@{} which collides with {}@{} required by {:?}",
                        package.name(),
                        found_version,
                        existing_package.name(),
                        existing_package.version(),
                        name,
                    );
                } else {
                    // Package not yet in the dependency graph, so we verify the version requirement
                    ensure!(
                        version_req.matches(found_version),
                        "a dependency of your project requires {}@{} but the resolved version is {}",
                        package.name(),
                        version_req,
                        found_version,
                    );
                }
            }

            package
        } else {
            // Non-root packages may not be physically present on disk.
            // Take it from the collected entries instead.
            deps.get(&dependency.package)
                .ok_or_else(|| {
                    miette::miette!(
                        "no resolved package found for local dependency {}",
                        dependency.package
                    )
                })?
                .package()
                .clone()
        };

        let dependency_name = package.name().clone();
        let sub_dependencies = package.manifest.dependencies.clone();
        let sub_dependency_names: Vec<_> = sub_dependencies
            .iter()
            .map(|sub_dependency| sub_dependency.package.clone())
            .collect();

        // Add the local package to the dependency graph
        deps.insert(
            dependency_name.clone(),
            ResolvedDependency::Local {
                package,
                path: abs_manifest_dir.clone(),
                dependants: vec![Dependant {
                    name,
                    version_req: VersionReq::STAR,
                }],
                depends_on: sub_dependency_names,
            },
        );

        // Process the sub-dependencies of the local package
        for sub_dependency in sub_dependencies {
            self.process_dependency(
                dependency_name.clone(),
                sub_dependency,
                false,
                &abs_manifest_dir,
                deps,
            )
            .await?;
        }

        Ok(())
    }

    #[async_recursion]
    async fn process_remote_dependency(
        &self,
        name: PackageName,
        dependency: RemoteDependency,
        is_root: bool,
        parent_dir: &Path,
        deps: &mut DependencyGraph,
    ) -> miette::Result<()> {
        let version_req = dependency.manifest.version.clone();

        // Check if the dependency is already resolved
        if let Some(entry) = deps.get_mut(&dependency.package) {
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
            // Resolve the dependency
            let dependency_pkg = self.resolve(dependency.clone(), is_root).await?;

            let dependency_name = dependency_pkg.name().clone();
            let sub_dependencies = dependency_pkg.manifest.dependencies.clone();
            let sub_dependency_names: Vec<_> = sub_dependencies
                .iter()
                .map(|sub_dependency| sub_dependency.package.clone())
                .collect();

            deps.insert(
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
                self.process_dependency(
                    dependency_name.clone(),
                    sub_dependency,
                    false,
                    parent_dir,
                    deps,
                )
                .await?;
            }
        }

        Ok(())
    }

    async fn resolve(
        &self,
        dependency: RemoteDependency,
        is_root: bool,
    ) -> miette::Result<Package> {
        if let Some(local_locked) = self.lockfile.get(&dependency.package) {
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
                if let Some(cached) = self.cache.get(local_locked.try_into()?).await? {
                    local_locked.validate(&cached)?;
                    return Ok(cached);
                }
            }

            let registry = Artifactory::new(
                dependency.manifest.registry.clone().try_into()?,
                self.credentials,
                self.policy,
            )
            .wrap_err(DownloadError {
                name: dependency.package.clone(),
                version: dependency.manifest.version.clone(),
            })?;

            let package = registry
                // TODO(#205): This works now because buffrs only supports pinned versions.
                // This logic has to change once we implement dynamic version resolution.
                .download(dependency.clone().into())
                .await
                .wrap_err(DownloadError {
                    name: dependency.package,
                    version: dependency.manifest.version,
                })?;

            let file_requirement = FileRequirement::try_from(local_locked)?;
            self.cache
                .put(file_requirement.into(), package.tgz.clone())
                .await
                .ok();

            Ok(package)
        } else {
            // Package not present in lockfile (and thus not in cache)
            // => download it from the registry
            let registry = Artifactory::new(
                dependency.manifest.registry.clone().try_into()?,
                self.credentials,
                self.policy,
            )
            .wrap_err(DownloadError {
                name: dependency.package.clone(),
                version: dependency.manifest.version.clone(),
            })?;

            let package = registry
                .download(dependency.clone().into())
                .await
                .wrap_err(DownloadError {
                    name: dependency.package,
                    version: dependency.manifest.version,
                })?;

            let key = Entry::from(&package);
            let content = package.tgz.clone();
            self.cache.put(key, content).await.ok();

            Ok(package)
        }
    }
}
