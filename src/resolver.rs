use std::{collections::HashMap, path::PathBuf, sync::Arc};

use async_recursion::async_recursion;
use miette::{Context, Diagnostic, bail, ensure, miette};
use semver::VersionReq;
use thiserror::Error;

use crate::package::PackageType;
use crate::{
    cache::{Cache, Entry},
    credentials::Credentials,
    lock::{FileRequirement, Lockfile},
    manifest::{
        Dependency, DependencyManifest, LocalDependencyManifest, MANIFEST_FILE, Manifest,
        RemoteDependencyManifest,
    },
    package::{Package, PackageName, PackageStore},
    registry::{Artifactory, RegistryUri},
};

/// Represents a dependency contextualized by the current dependency graph
#[derive(Debug)]
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
#[derive(Debug)]
pub struct Dependant {
    /// Package that requested the dependency
    pub name: PackageName,
    /// Version requirement
    pub version_req: VersionReq,
}

/// Represents direct and transitive dependencies of the root package
#[derive(Debug)]
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

struct ProcessDependency<'a> {
    name: PackageName,
    dependency: Dependency,
    parent_package_type: Option<PackageType>,
    is_root: bool,
    lockfile: &'a Lockfile,
    credentials: &'a Arc<Credentials>,
    cache: &'a Cache,
    preserve_mtime: bool,
    base_path: &'a PathBuf,
}

struct ProcessLocalDependency<'a> {
    name: PackageName,
    dependency: LocalDependency,
    parent_package_type: Option<PackageType>,
    #[allow(dead_code)]
    is_root: bool,
    lockfile: &'a Lockfile,
    credentials: &'a Arc<Credentials>,
    cache: &'a Cache,
    preserve_mtime: bool,
    base_path: &'a PathBuf,
}

struct ProcessRemoteDependency<'a> {
    name: PackageName,
    dependency: RemoteDependency,
    parent_package_type: Option<PackageType>,
    is_root: bool,
    lockfile: &'a Lockfile,
    credentials: &'a Arc<Credentials>,
    cache: &'a Cache,
    preserve_mtime: bool,
    base_path: &'a PathBuf,
}

impl DependencyGraph {
    /// Recursively resolves dependencies from the manifest to build a dependency graph
    pub async fn from_manifest(
        manifest: &Manifest,
        lockfile: &Lockfile,
        credentials: &Arc<Credentials>,
        cache: &Cache,
        preserve_mtime: bool,
        base_path: &PathBuf,
    ) -> miette::Result<Self> {
        let parent_package_type = manifest.clone().package.map(|p| p.kind);

        let name = manifest
            .package
            .as_ref()
            .map(|p| p.name.clone())
            .unwrap_or_else(|| PackageName::unchecked("."));

        let mut entries = HashMap::new();

        for dependency in manifest.dependencies.iter().flatten() {
            Self::process_dependency(
                &mut entries,
                ProcessDependency {
                    name: name.clone(),
                    dependency: dependency.clone(),
                    parent_package_type,
                    is_root: true,
                    lockfile,
                    credentials,
                    cache,
                    preserve_mtime,
                    base_path,
                },
            )
            .await?;
        }

        Ok(Self { entries })
    }

    async fn process_dependency(
        entries: &mut HashMap<PackageName, ResolvedDependency>,
        params: ProcessDependency<'_>,
    ) -> miette::Result<()> {
        let ProcessDependency {
            name,
            dependency,
            parent_package_type,
            is_root,
            lockfile,
            credentials,
            cache,
            preserve_mtime,
            base_path,
        } = params;
        match dependency.manifest {
            DependencyManifest::Remote(manifest) => {
                Self::process_remote_dependency(
                    entries,
                    ProcessRemoteDependency {
                        name: name.clone(),
                        dependency: RemoteDependency {
                            package: dependency.package,
                            manifest,
                        },
                        parent_package_type,
                        is_root,
                        lockfile,
                        credentials,
                        cache,
                        preserve_mtime,
                        base_path,
                    },
                )
                .await?;
            }
            DependencyManifest::Local(manifest) => {
                Self::process_local_dependency(
                    entries,
                    ProcessLocalDependency {
                        name: name.clone(),
                        dependency: LocalDependency {
                            package: dependency.package,
                            manifest,
                        },
                        parent_package_type,
                        is_root,
                        lockfile,
                        credentials,
                        cache,
                        preserve_mtime,
                        base_path,
                    },
                )
                .await?;
            }
        }

        Ok(())
    }

    #[async_recursion]
    async fn process_local_dependency<'a>(
        entries: &'a mut HashMap<PackageName, ResolvedDependency>,
        params: ProcessLocalDependency<'a>,
    ) -> miette::Result<()> {
        let ProcessLocalDependency {
            name,
            dependency,
            parent_package_type,
            is_root: _,
            lockfile,
            credentials,
            cache,
            preserve_mtime,
            base_path,
        } = params;

        // Resolve the dependency path relative to the base path (package directory)
        let resolved_path = base_path.join(&dependency.manifest.path);

        let path = resolved_path.join(MANIFEST_FILE);

        let manifest = Manifest::try_read_from(&path).await.wrap_err({
            miette::miette!(
                "no `{}` for package {} found at path {}",
                MANIFEST_FILE,
                dependency.package,
                path.display()
            )
        })?;

        let store = PackageStore::open(&resolved_path).await?;
        let package = store.release(&manifest, preserve_mtime).await?;
        let dependency_name = package.name().clone();

        let package_type = package.clone().manifest.package.map(|p| p.kind);

        if let (Some(PackageType::Lib), Some(PackageType::Api)) =
            (parent_package_type, package_type)
        {
            return Err(miette!(
                "A package of type lib can not be dependent on type api"
            ));
        }

        let sub_dependencies = package.manifest.dependencies.clone();
        let sub_dependency_names: Vec<_> = sub_dependencies
            .iter()
            .flatten()
            .map(|sub_dependency| sub_dependency.package.clone())
            .collect();

        entries.insert(
            dependency_name.clone(),
            ResolvedDependency::Local {
                package,
                path: resolved_path.clone(),
                dependants: vec![Dependant {
                    name,
                    version_req: VersionReq::STAR,
                }],
                depends_on: sub_dependency_names,
            },
        );

        for sub_dependency in sub_dependencies.into_iter().flatten() {
            Self::process_dependency(
                entries,
                ProcessDependency {
                    name: dependency_name.clone(),
                    dependency: sub_dependency,
                    parent_package_type: package_type,
                    is_root: false,
                    lockfile,
                    credentials,
                    cache,
                    preserve_mtime,
                    base_path: &resolved_path,
                },
            )
            .await?;
        }

        Ok(())
    }

    #[async_recursion]
    async fn process_remote_dependency<'a>(
        entries: &'a mut HashMap<PackageName, ResolvedDependency>,
        params: ProcessRemoteDependency<'a>,
    ) -> miette::Result<()> {
        let ProcessRemoteDependency {
            name,
            dependency,
            parent_package_type,
            is_root,
            lockfile,
            credentials,
            cache,
            preserve_mtime,
            base_path,
        } = params;
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

            let package_type = dependency_pkg.clone().manifest.package.map(|p| p.kind);

            if let (Some(PackageType::Lib), Some(PackageType::Api)) =
                (parent_package_type, package_type)
            {
                return Err(miette!(
                    "A package of type lib can not be dependent on type api"
                ));
            }

            let dependency_name = dependency_pkg.name().clone();
            let sub_dependencies = dependency_pkg.manifest.dependencies.clone();
            let sub_dependency_names: Vec<_> = sub_dependencies
                .iter()
                .flatten()
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

            for sub_dependency in sub_dependencies.into_iter().flatten() {
                Self::process_dependency(
                    entries,
                    ProcessDependency {
                        name: dependency_name.clone(),
                        dependency: sub_dependency,
                        parent_package_type: package_type,
                        is_root: false,
                        lockfile,
                        credentials,
                        cache,
                        preserve_mtime,
                        base_path,
                    },
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

            let registry = Artifactory::new(dependency.manifest.registry.clone(), credentials)
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

            let file_requirement = FileRequirement::from(local_locked);
            cache
                .put(file_requirement.into(), package.tgz.clone())
                .await
                .ok();

            Ok(package)
        } else {
            let registry = Artifactory::new(dependency.manifest.registry.clone(), credentials)
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
            cache.put(key, content).await.ok();

            Ok(package)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::{Manifest, PackageManifest};
    use semver::Version;
    use std::sync::Arc;
    use tempfile::TempDir;

    fn create_test_manifest(
        name: &str,
        package_type: PackageType,
        dependencies: Vec<Dependency>,
    ) -> Manifest {
        Manifest::builder()
            .package(PackageManifest {
                kind: package_type,
                name: name.parse().unwrap(),
                version: Version::new(0, 1, 0),
                description: None,
            })
            .dependencies(dependencies)
            .build()
    }

    #[tokio::test]
    async fn test_lib_cannot_depend_on_api() {
        let temp_dir = TempDir::new().unwrap();
        let api_dir = temp_dir.path().join("api-package");
        std::fs::create_dir(&api_dir).unwrap();

        // Create an API package
        let api_manifest = create_test_manifest("api-package", PackageType::Api, vec![]);
        api_manifest.write_at(&api_dir).await.unwrap();

        // Create proto directory for API package
        std::fs::create_dir_all(api_dir.join("proto")).unwrap();

        // Create a lib package that tries to depend on the API package
        let lib_manifest = Manifest::builder()
            .package(PackageManifest {
                kind: PackageType::Lib,
                name: "lib-package".parse().unwrap(),
                version: Version::new(0, 1, 0),
                description: None,
            })
            .dependencies(vec![Dependency {
                package: "api-package".parse().unwrap(),
                manifest: LocalDependencyManifest {
                    path: api_dir.clone(),
                }
                .into(),
            }])
            .build();

        let lockfile = Lockfile::default();
        let credentials = Arc::new(Credentials::default());
        let cache = Cache::open().await.unwrap();

        let result = DependencyGraph::from_manifest(
            &lib_manifest,
            &lockfile,
            &credentials,
            &cache,
            false,
            &temp_dir.path().to_path_buf(),
        )
        .await;

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("lib can not be dependent on type api")
        );
    }

    #[tokio::test]
    async fn test_api_can_depend_on_lib() {
        let temp_dir = TempDir::new().unwrap();
        let lib_dir = temp_dir.path().join("lib-package");
        std::fs::create_dir(&lib_dir).unwrap();

        // Create a lib package
        let lib_manifest = create_test_manifest("lib-package", PackageType::Lib, vec![]);
        lib_manifest.write_at(&lib_dir).await.unwrap();

        // Create proto directory for lib package
        std::fs::create_dir_all(lib_dir.join("proto")).unwrap();

        // Create an API package that depends on the lib package (should work)
        let api_manifest = Manifest::builder()
            .package(PackageManifest {
                kind: PackageType::Api,
                name: "api-package".parse().unwrap(),
                version: Version::new(0, 1, 0),
                description: None,
            })
            .dependencies(vec![Dependency {
                package: "lib-package".parse().unwrap(),
                manifest: LocalDependencyManifest {
                    path: lib_dir.clone(),
                }
                .into(),
            }])
            .build();

        let lockfile = Lockfile::default();
        let credentials = Arc::new(Credentials::default());
        let cache = Cache::open().await.unwrap();

        let result = DependencyGraph::from_manifest(
            &api_manifest,
            &lockfile,
            &credentials,
            &cache,
            false,
            &temp_dir.path().to_path_buf(),
        )
        .await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_empty_dependencies_graph() {
        let manifest = create_test_manifest("test-package", PackageType::Lib, vec![]);
        let lockfile = Lockfile::default();
        let credentials = Arc::new(Credentials::default());
        let cache = Cache::open().await.unwrap();
        let temp_dir = TempDir::new().unwrap();

        let graph = DependencyGraph::from_manifest(
            &manifest,
            &lockfile,
            &credentials,
            &cache,
            false,
            &temp_dir.path().to_path_buf(),
        )
        .await
        .unwrap();

        assert_eq!(graph.entries.len(), 0);
    }
}
