// (c) Copyright 2025 Helsing GmbH. All rights reserved.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use async_trait::async_trait;
use miette::{Context as _, IntoDiagnostic, bail, ensure};
use semver::VersionReq;

use crate::io::File;
use crate::lock::{DigestAlgorithm, LockedDependency};
use crate::{
    cache::{Cache, Entry as CacheEntry},
    credentials::Credentials,
    lock::{LOCKFILE, LockedPackage, Lockfile, PackageLockfile, WorkspaceLockfile},
    manifest::{
        Manifest,
        package::{Dependency, DependencyManifest, PackagesManifest, RemoteDependencyManifest},
        workspace::WorkspaceManifest,
    },
    package::{Package, PackageName, PackageStore},
    registry::{Artifactory, RegistryUri},
    resolver::{DependencyError, DependencyGraph, DependencySource},
};

/// Trait for types that can install their dependencies
#[async_trait]
pub trait Install {
    /// Installs dependencies and returns the resulting locked packages
    async fn install(&self, ctx: &InstallationContext) -> miette::Result<Vec<LockedPackage>>;
}

/// A resolved remote package with its registry metadata
#[derive(Debug, Clone)]
struct ResolvedRemotePackage {
    package: Package,
    registry: RegistryUri,
    repository: String,
}

/// Controls whether network requests are allowed during installation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkMode {
    /// Allow network requests (default behavior).
    Online,
    /// Do not make any network requests; all packages must be in the local cache.
    Offline,
}

/// Context carrying shared state for package installation
#[derive(Debug, Clone)]
pub struct InstallationContext {
    cwd: PathBuf,
    credentials: Credentials,
    cache: Cache,
    store: PackageStore,
    lock: Lockfile,
    preserve_mtime: bool,
    network_mode: NetworkMode,
}

impl InstallationContext {
    /// Creates a new installation context rooted at the given directory
    pub async fn new(
        cwd: impl AsRef<Path>,
        preserve_mtime: bool,
        network_mode: NetworkMode,
    ) -> miette::Result<Self> {
        let cwd = cwd.as_ref().to_path_buf();

        let credentials = Credentials::load().await?;

        let cache = Cache::open().await?;

        let store = PackageStore::open(&cwd).await?;

        let lock = Lockfile::load_from_or_infer(&cwd).await?;

        Ok(Self {
            cwd,
            credentials,
            cache,
            store,
            lock,
            preserve_mtime,
            network_mode,
        })
    }

    /// Returns a new child context for a workspace member, opening its own package store
    async fn child(&self, cwd: PathBuf) -> miette::Result<Self> {
        let store = PackageStore::open(&cwd).await?;

        Ok(Self {
            cwd,
            store,
            ..self.clone()
        })
    }

    /// Creates a new installation context rooted at the current working directory
    pub async fn cwd(preserve_mtime: bool, network_mode: NetworkMode) -> miette::Result<Self> {
        let cwd = std::env::current_dir().into_diagnostic()?;

        Self::new(cwd, preserve_mtime, network_mode).await
    }
}

#[async_trait]
impl Install for Manifest {
    async fn install(&self, ctx: &InstallationContext) -> miette::Result<Vec<LockedPackage>> {
        match self {
            Manifest::Package(pkg) => pkg.install(ctx).await,
            Manifest::Workspace(wrk) => wrk.install(ctx).await,
        }
    }
}

#[async_trait]
impl Install for PackagesManifest {
    async fn install(&self, ctx: &InstallationContext) -> miette::Result<Vec<LockedPackage>> {
        // 1. Clear the package store
        ctx.store.clear().await?;

        // 2. Install the current package
        if let Some(ref pkg) = self.package {
            ctx.store.populate(pkg).await?;

            tracing::info!("installed {}@{}", pkg.name, pkg.version);
        }

        // 3. Build the dependency graph
        let graph = DependencyGraph::build(
            &self,
            &ctx.cwd,
            &ctx.credentials,
            Some(ctx.lock.clone()),
            ctx.network_mode,
        )
        .await?;

        let dependencies = graph.ordered_dependencies()?;

        // 4. Install all dependencies and track resolved remote packages
        let mut remote: HashMap<PackageName, ResolvedRemotePackage> = HashMap::new();

        for dependency in dependencies {
            let package = match dependency.node.source {
                // 4.a. Install local dependencies by publishing and unpacking
                DependencySource::Local { path } => {
                    let manifest = Manifest::require_package_manifest(&path).await?;

                    let store = PackageStore::open(path).await?;

                    store.release(&manifest, ctx.preserve_mtime).await?
                }
                // 4.b. Install remote dependencies by downloading
                DependencySource::Remote {
                    registry,
                    repository,
                } => {
                    let name = &dependency.node.name;
                    let version = &dependency.node.version;

                    let installed =
                        utils::install(&registry, &repository, name, version, ctx).await?;

                    // 4.b.1. Track this resolved remote package
                    remote.insert(
                        name.clone(),
                        ResolvedRemotePackage {
                            package: installed.clone(),
                            registry: registry.clone(),
                            repository: repository.clone(),
                        },
                    );

                    installed
                }
            };

            ctx.store
                .unpack(&package)
                .await
                .wrap_err_with(|| format!("failed to unpack package {}", package.name()))?;

            tracing::info!("installed {}@{}", dependency.name, package.version());
        }

        // 5. Lock packages with dependency information
        let mut locked = Vec::new();

        for (name, resolved) in &remote {
            let node = graph
                .nodes
                .get(name)
                .ok_or_else(|| miette::miette!("Package {name} not found in dependency graph"))?;

            // 5.1 Map dependency names to their resolved versions (only remote ones)
            let deps: Vec<LockedDependency> = node
                .dependencies
                .iter()
                .filter_map(|name| {
                    remote.get(name).map(|resolved| {
                        LockedDependency::qualified(
                            name.clone(),
                            resolved.package.version().clone(),
                        )
                    })
                })
                .collect();

            // 5.2 Create WorkspaceLockedPackage with dependencies
            locked.push(LockedPackage {
                name: resolved.package.name().clone(),
                version: resolved.package.version().clone(),
                digest: DigestAlgorithm::SHA256.digest(&resolved.package.tgz),
                registry: resolved.registry.clone(),
                repository: resolved.repository.clone(),
                dependencies: deps,
                // NOTE: For workspace lockfiles, set dependants=1 (this package needs it)
                // Aggregation will sum them up across all workspace members
                //
                // TODO(mara): revisit
                dependants: 1,
            });
        }

        // 6. Write lockfile if context is a package lockfile
        if ctx.lock.is_package_lockfile() {
            let lock: PackageLockfile = locked.clone().try_into()?;
            lock.save_to(&ctx.cwd).await?;
        }

        Ok(locked)
    }
}

#[async_trait]
impl Install for WorkspaceManifest {
    async fn install(&self, ctx: &InstallationContext) -> miette::Result<Vec<LockedPackage>> {
        let packages = self.workspace.members(&ctx.cwd)?;

        tracing::info!(
            "workspace found. running install for {} packages in workspace",
            packages.len()
        );

        // 1. Install all workspace member and collect locked packages
        let mut locked = vec![];

        for package in packages {
            let manifest = Manifest::require_package_manifest(&package).await?;

            if PackageLockfile::exists_at(&package).await? {
                tracing::warn!(
                    "[warn] package lockfile found at {}. Consider removing it - workspace installs use workspace-level lockfile",
                    PackageLockfile::resolve(&package)?.display()
                );
            }

            tracing::info!("running install for package: {}", package.display());

            let member_ctx = ctx.child(ctx.cwd.join(&package)).await?;
            let new = manifest.install(&member_ctx).await?;

            locked.extend_from_slice(&new);
        }

        tracing::info!("workspace install complete using existing lockfile");

        // 2. Write lockfile if context is a workspace lockfile
        if ctx.lock.is_workspace_lockfile() {
            let lock: WorkspaceLockfile = locked.clone().try_into()?;

            lock.save_to(&ctx.cwd).await?;

            tracing::info!(
                "wrote workspace lockfile at {}",
                ctx.cwd.join(LOCKFILE).display()
            );
        }

        Ok(locked)
    }
}

mod utils {
    use super::*;

    /// Finds a matching version in the workspace lockfile
    pub fn find_matching_workspace_locked<'a>(
        lockfile: &'a Lockfile,
        name: &PackageName,
        requirement: &VersionReq,
    ) -> Option<&'a LockedPackage> {
        lockfile
            .packages()
            .filter(|pkg| pkg.name == *name)
            .filter(|pkg| requirement.matches(&pkg.version))
            .max_by_key(|pkg| &pkg.version) // Highest matching version (tiebreaker)
    }

    /// Downloads the exact version specified
    pub async fn download_exact(
        package_name: &PackageName,
        registry: &RegistryUri,
        repository: &str,
        version: &semver::Version,
        ctx: &InstallationContext,
    ) -> miette::Result<Package> {
        let version_req = VersionReq::parse(&format!("={}", version))
            .into_diagnostic()
            .wrap_err("failed to create exact version requirement")?;

        download(package_name, registry, repository, &version_req, ctx).await
    }

    /// Downloads a package from the registry and caches it.
    ///
    /// Returns an error if `ctx.network_mode` is [`NetworkMode::Offline`].
    pub async fn download(
        package_name: &PackageName,
        registry: &RegistryUri,
        repository: &str,
        version: &VersionReq,
        ctx: &InstallationContext,
    ) -> miette::Result<Package> {
        if ctx.network_mode == NetworkMode::Offline {
            bail!(DependencyError::Offline {
                name: package_name.clone(),
                version: version.clone(),
            });
        }

        // 1. Download the package from artifactory
        let artifactory = Artifactory::new(registry.clone(), &ctx.credentials)
            .wrap_err_with(|| format!("failed to initialize registry {}", registry))?;

        let resolved_version = artifactory
            .resolve_version(repository.to_string(), package_name.clone(), version)
            .await
            .wrap_err_with(|| {
                format!(
                    "could not resolve {}@{} from registry {}",
                    package_name, version, registry
                )
            })?;

        let dependency = Dependency {
            package: package_name.clone(),
            manifest: DependencyManifest::Remote(RemoteDependencyManifest {
                registry: registry.clone(),
                repository: repository.to_string(),
                version: version.clone(),
            }),
        };

        let downloaded_package = artifactory.download(dependency, &resolved_version).await?;

        // 2. Cache the downloaded package for future installs
        let cache_key = CacheEntry::from(&downloaded_package);

        ctx.cache
            .put(cache_key, downloaded_package.tgz.clone())
            .await
            .ok();

        Ok(downloaded_package)
    }

    pub async fn install(
        registry: &RegistryUri,
        repository: &str,
        name: &PackageName,
        version: &VersionReq,
        ctx: &InstallationContext,
    ) -> miette::Result<Package> {
        let Some(locked) = utils::find_matching_workspace_locked(&ctx.lock, name, version) else {
            return utils::download(name, registry, repository, version, ctx).await;
        };

        ensure!(
            registry == &locked.registry,
            "registry mismatch for {}: manifest specifies {} but workspace lockfile requires {}",
            name,
            registry,
            locked.registry
        );

        tracing::debug!(
            "using locked packaged version for {}@{}",
            name,
            locked.version
        );

        // Try to get from cache
        if let Some(cached) = ctx.cache.get(locked.clone().into()).await? {
            locked.validate(&cached)?;

            return Ok(cached);
        }

        utils::download_exact(name, registry, repository, &locked.version, ctx).await
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use crate::lock::{Digest, DigestAlgorithm, LockedPackage};
        use semver::Version;
        use std::collections::HashMap;
        use std::str::FromStr;
        use tempfile::TempDir;

        fn create_lockfile_with_package(
            package_name: &str,
            version: &str,
            registry: &str,
            repository: &str,
        ) -> PackageLockfile {
            let locked_pkg = LockedPackage {
                name: PackageName::unchecked(package_name),
                version: Version::parse(version).unwrap(),
                registry: RegistryUri::from_str(registry).unwrap(),
                repository: repository.to_string(),
                digest: "sha256:0000000000000000000000000000000000000000000000000000000000000000"
                    .parse()
                    .unwrap(),
                dependencies: Default::default(),
                dependants: 1,
            };
            PackageLockfile::from_iter(vec![locked_pkg])
        }

        #[tokio::test]
        async fn test_registry_mismatch_fails() {
            let tmp = TempDir::new().unwrap();

            let lockfile =
                create_lockfile_with_package("test-pkg", "1.0.0", "https://registry-a.com", "repo");

            // Construct InstallationContext directly to test utils::install independently
            // of DependencyGraph::build (which eagerly downloads packages)
            let ctx = InstallationContext {
                cwd: tmp.path().to_path_buf(),
                credentials: Credentials {
                    registry_tokens: HashMap::new(),
                },
                cache: Cache::open().await.unwrap(),
                store: PackageStore::open(tmp.path()).await.unwrap(),
                lock: Lockfile::Package(lockfile),
                preserve_mtime: false,
                network_mode: NetworkMode::Online,
            };

            let pkg_name = PackageName::unchecked("test-pkg");
            let registry_b = RegistryUri::from_str("https://registry-b.com").unwrap();
            let version = VersionReq::parse("=1.0.0").unwrap();

            let result = install(&registry_b, "repo", &pkg_name, &version, &ctx).await;

            // Should fail with registry mismatch error
            assert!(result.is_err(), "expected error but got success");
            let err_msg = format!("{:?}", result.unwrap_err());
            assert!(
                err_msg.contains("registry mismatch"),
                "expected 'registry mismatch' in error, got: {}",
                err_msg
            );
        }

        #[tokio::test]
        async fn test_online_mode_allows_download() {
            let tmp = TempDir::new().unwrap();

            // Create a context with Online mode and no lockfile
            let ctx = InstallationContext {
                cwd: tmp.path().to_path_buf(),
                credentials: Credentials {
                    registry_tokens: HashMap::new(),
                },
                cache: Cache::open().await.unwrap(),
                store: PackageStore::open(tmp.path()).await.unwrap(),
                lock: Lockfile::Package(PackageLockfile::default()),
                preserve_mtime: false,
                network_mode: NetworkMode::Online,
            };

            let pkg_name = PackageName::unchecked("test-pkg");
            let registry = RegistryUri::from_str("https://registry.example.com").unwrap();
            let version = VersionReq::parse("=1.0.0").unwrap();

            // Attempt download in online mode (will fail due to no actual registry, but should not fail with offline error)
            let result = download(&pkg_name, &registry, "repo", &version, &ctx).await;

            // Should fail with connection/registry error, not offline error
            assert!(result.is_err(), "expected error due to no actual registry");
            let err_msg = format!("{:?}", result.unwrap_err());
            assert!(
                !err_msg.contains("offline"),
                "should not fail with offline error in online mode, got: {}",
                err_msg
            );
        }

        #[tokio::test]
        async fn test_offline_mode_prevents_download() {
            let tmp = TempDir::new().unwrap();

            // Create a context with Offline mode
            let ctx = InstallationContext {
                cwd: tmp.path().to_path_buf(),
                credentials: Credentials {
                    registry_tokens: HashMap::new(),
                },
                cache: Cache::open().await.unwrap(),
                store: PackageStore::open(tmp.path()).await.unwrap(),
                lock: Lockfile::Package(PackageLockfile::default()),
                preserve_mtime: false,
                network_mode: NetworkMode::Offline,
            };

            let pkg_name = PackageName::unchecked("test-pkg");
            let registry = RegistryUri::from_str("https://registry.example.com").unwrap();
            let version = VersionReq::parse("=1.0.0").unwrap();

            // Attempt download in offline mode
            let result = download(&pkg_name, &registry, "repo", &version, &ctx).await;

            // Should fail with offline error
            assert!(result.is_err(), "expected error in offline mode");
            let err_msg = format!("{:?}", result.unwrap_err());
            assert!(
                err_msg.contains("offline"),
                "expected 'offline' in error message, got: {}",
                err_msg
            );
        }

        #[test]
        fn test_find_matching_workspace_locked() {
            // Create workspace lockfile with multiple versions
            let pkg_v1 = LockedPackage {
                name: PackageName::unchecked("remote-lib"),
                version: Version::new(1, 5, 0),
                digest: Digest::from_parts(
                    DigestAlgorithm::SHA256,
                    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                )
                .unwrap(),
                registry: RegistryUri::from_str("https://registry.com").unwrap(),
                repository: "test-repo".to_string(),
                dependencies: vec![],
                dependants: 1,
            };

            let pkg_v2 = LockedPackage {
                name: PackageName::unchecked("remote-lib"),
                version: Version::new(2, 0, 0),
                digest: Digest::from_parts(
                    DigestAlgorithm::SHA256,
                    "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                )
                .unwrap(),
                registry: RegistryUri::from_str("https://registry.com").unwrap(),
                repository: "test-repo".to_string(),
                dependencies: vec![],
                dependants: 1,
            };

            let lockfile = Lockfile::Workspace(WorkspaceLockfile::from_iter(vec![pkg_v1, pkg_v2]));

            // Test finding version matching ^1.0.0
            let req_v1 = VersionReq::parse("^1.0.0").unwrap();
            let found = find_matching_workspace_locked(
                &lockfile,
                &PackageName::unchecked("remote-lib"),
                &req_v1,
            );
            assert!(found.is_some());
            assert_eq!(found.unwrap().version, Version::new(1, 5, 0));

            // Test finding version matching ^2.0.0
            let req_v2 = VersionReq::parse("^2.0.0").unwrap();
            let found = find_matching_workspace_locked(
                &lockfile,
                &PackageName::unchecked("remote-lib"),
                &req_v2,
            );
            assert!(found.is_some());
            assert_eq!(found.unwrap().version, Version::new(2, 0, 0));

            // Test not finding version matching ^3.0.0
            let req_v3 = VersionReq::parse("^3.0.0").unwrap();
            let found = find_matching_workspace_locked(
                &lockfile,
                &PackageName::unchecked("remote-lib"),
                &req_v3,
            );
            assert!(found.is_none());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lock::LockedPackage;
    use semver::Version;
    use std::str::FromStr;

    #[test]
    fn test_aggregate_workspace_lockfile_multiple_versions() {
        use crate::lock::{Digest, DigestAlgorithm};

        // Create two different versions of the same package from different members
        let pkg_v1 = LockedPackage {
            name: PackageName::unchecked("remote-lib"),
            version: Version::new(1, 0, 0),
            digest: Digest::from_parts(
                DigestAlgorithm::SHA256,
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            )
            .unwrap(),
            registry: RegistryUri::from_str("https://registry.com").unwrap(),
            repository: "test-repo".to_string(),
            dependencies: vec![],
            dependants: 1,
        };

        let pkg_v2 = LockedPackage {
            name: PackageName::unchecked("remote-lib"),
            version: Version::new(2, 0, 0),
            digest: Digest::from_parts(
                DigestAlgorithm::SHA256,
                "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            )
            .unwrap(),
            registry: RegistryUri::from_str("https://registry.com").unwrap(),
            repository: "test-repo".to_string(),
            dependencies: vec![],
            dependants: 1,
        };

        // Also add the same version from two different members
        let pkg_v1_dup = LockedPackage {
            name: PackageName::unchecked("remote-lib"),
            version: Version::new(1, 0, 0),
            registry: RegistryUri::from_str("https://registry.com").unwrap(),
            repository: "test-repo".to_string(),
            digest: Digest::from_parts(
                DigestAlgorithm::SHA256,
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            )
            .unwrap(),
            dependencies: vec![],
            dependants: 1,
        };

        let locked_packages = vec![pkg_v1, pkg_v2, pkg_v1_dup];

        let workspace_lockfile = WorkspaceLockfile::try_from(locked_packages).unwrap();

        // Should have 2 entries (v1.0.0 and v2.0.0), not 3
        assert_eq!(workspace_lockfile.packages().count(), 2);

        // v1.0.0 should have dependants=2 (merged from two members)
        let v1 = workspace_lockfile
            .get(
                &PackageName::unchecked("remote-lib"),
                &Version::new(1, 0, 0),
            )
            .expect("v1.0.0 should exist");
        assert_eq!(v1.version, Version::new(1, 0, 0));
        assert_eq!(v1.dependants, 2); // Summed from two members

        // v2.0.0 should have dependants=1
        let v2 = workspace_lockfile
            .get(
                &PackageName::unchecked("remote-lib"),
                &Version::new(2, 0, 0),
            )
            .expect("v2.0.0 should exist");
        assert_eq!(v2.version, Version::new(2, 0, 0));
        assert_eq!(v2.dependants, 1);
    }
}
