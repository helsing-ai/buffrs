// (c) Copyright 2025 Helsing GmbH. All rights reserved.

use miette::{Context as _, IntoDiagnostic, ensure};
use semver::VersionReq;
use std::collections::HashMap;
use std::{
    env,
    path::{Path, PathBuf},
};

use crate::lock::{DigestAlgorithm, LockedDependency};
use crate::{
    cache::{Cache, Entry as CacheEntry},
    credentials::Credentials,
    lock::{LOCKFILE, Lockfile, PackageLockfile, WorkspaceLockedPackage, WorkspaceLockfile},
    manifest::{
        BuffrsManifest, Dependency, DependencyManifest, MANIFEST_FILE, PackagesManifest,
        RemoteDependencyManifest, WorkspaceManifest,
    },
    package::{Package, PackageName, PackageStore},
    registry::{Artifactory, RegistryUri},
    resolver::{DependencyGraph, DependencySource},
};

/// Controls whether the lockfile is written to disk during package installation
#[derive(Debug, Clone, Copy)]
enum LockfileWriteMode {
    /// Write the lockfile to the package directory (for standalone package installs)
    Write,
    /// Skip writing the lockfile (for workspace members - workspace handles lockfile)
    Skip,
}

/// Controls workspace lockfile usage during package installation
#[derive(Debug, Clone, Copy)]
enum WorkspaceLockfileMode<'a> {
    /// Use an existing workspace lockfile for version resolution
    UseExisting(&'a WorkspaceLockfile),
    /// No workspace lockfile available (resolve from registry)
    CreateNew,
}

/// A resolved remote package with its registry metadata
#[derive(Debug, Clone)]
struct ResolvedRemotePackage {
    package: Package,
    registry: RegistryUri,
    repository: String,
}

/// Helper for installing package dependencies
///
/// Encapsulates the installation logic including cache management,
/// lockfile handling, and dependency resolution.
pub struct Installer {
    preserve_mtime: bool,
    credentials: Credentials,
    cache: Cache,
}

impl Installer {
    /// Creates a new Installer instance
    pub async fn new(preserve_mtime: bool) -> miette::Result<Self> {
        let credentials = Credentials::load().await?;
        let cache = Cache::open().await?;

        Ok(Self {
            preserve_mtime,
            credentials,
            cache,
        })
    }

    /// Installs dependencies for the current project
    ///
    /// Behavior depends on the manifest type:
    /// - **Package**: Installs dependencies listed in the `[dependencies]` section
    /// - **Workspace**: Installs dependencies for all workspace members
    pub async fn install(&self, manifest: &BuffrsManifest) -> miette::Result<()> {
        match manifest {
            BuffrsManifest::Package(packages_manifest) => {
                let lockfile = PackageLockfile::read_or_default().await?;
                let store = PackageStore::current().await?;
                let current_path = env::current_dir()
                    .into_diagnostic()
                    .wrap_err("current dir could not be retrieved")?;

                self.install_package(
                    packages_manifest,
                    &lockfile,
                    &store,
                    &current_path,
                    WorkspaceLockfileMode::CreateNew,
                    LockfileWriteMode::Write,
                )
                .await?;

                Ok(())
            }
            BuffrsManifest::Workspace(workspace_manifest) => {
                self.install_workspace(workspace_manifest).await
            }
        }
    }

    async fn install_workspace(&self, manifest: &WorkspaceManifest) -> miette::Result<()> {
        let root_path = env::current_dir()
            .into_diagnostic()
            .wrap_err("current dir could not be retrieved")?;

        let workspace_lockfile_path = root_path.join(LOCKFILE);

        if WorkspaceLockfile::exists_at(&workspace_lockfile_path).await? {
            tracing::info!(":: using existing workspace lockfile");
            let workspace_lockfile = WorkspaceLockfile::read_from(&workspace_lockfile_path).await?;
            self.install_with_workspace_lockfile(manifest, &root_path, &workspace_lockfile)
                .await
        } else {
            tracing::info!(
                ":: no workspace lockfile found, installing workspace members & creating new one"
            );
            self.install_and_create_workspace_lockfile(manifest, &root_path)
                .await
        }
    }

    /// Installs workspace dependencies using an existing workspace lockfile
    async fn install_with_workspace_lockfile(
        &self,
        manifest: &WorkspaceManifest,
        root_path: &PathBuf,
        workspace_lockfile: &WorkspaceLockfile,
    ) -> miette::Result<()> {
        let packages = manifest.workspace.resolve_members(root_path)?;
        tracing::info!(
            ":: workspace found. running install for {} packages in workspace",
            packages.len()
        );

        for package in packages {
            let pkg_manifest =
                BuffrsManifest::require_package_manifest(&package.join(MANIFEST_FILE)).await?;

            // Warn if package lockfile exists
            let pkg_lockfile_path = package.join(LOCKFILE);
            if PackageLockfile::exists_at(&pkg_lockfile_path).await? {
                tracing::warn!(
                    "[WARN] package lockfile found at {}. Consider removing it - workspace installs use workspace-level lockfile",
                    pkg_lockfile_path.display()
                );
            }

            let store = PackageStore::open(&package).await?;

            tracing::info!(":: running install for package: {}", package.display());

            // Install using workspace lockfile (no package lockfile needed)
            self.install_package(
                &pkg_manifest,
                &PackageLockfile::default(),
                &store,
                &package,
                WorkspaceLockfileMode::UseExisting(workspace_lockfile),
                LockfileWriteMode::Skip,
            )
            .await?;
        }

        tracing::info!(":: workspace install complete using existing lockfile");

        Ok(())
    }

    /// Installs workspace dependencies and creates a new workspace lockfile
    async fn install_and_create_workspace_lockfile(
        &self,
        manifest: &WorkspaceManifest,
        root_path: &PathBuf,
    ) -> miette::Result<()> {
        let packages = manifest.workspace.resolve_members(root_path)?;
        tracing::info!(
            ":: workspace found. running install for {} packages in workspace",
            packages.len()
        );

        let mut all_locked_packages = Vec::new();

        for package in packages {
            let pkg_manifest =
                BuffrsManifest::require_package_manifest(&package.join(MANIFEST_FILE)).await?;

            // WARN if package lockfile exists
            let pkg_lockfile_path = package.join(LOCKFILE);
            if PackageLockfile::exists_at(&pkg_lockfile_path).await? {
                tracing::warn!(
                    "package lockfile found at {}. Consider removing it - workspace installs use workspace-level lockfile",
                    pkg_lockfile_path.display()
                );
            }

            let pkg_lockfile = PackageLockfile::read_from_or_default(&pkg_lockfile_path).await?;
            let store = PackageStore::open(&package).await?;

            tracing::info!(":: running install for package: {}", package.display());

            // Install without workspace lockfile (resolve from registry)
            let locked = self
                .install_package(
                    &pkg_manifest,
                    &pkg_lockfile,
                    &store,
                    &package,
                    WorkspaceLockfileMode::CreateNew,
                    LockfileWriteMode::Skip,
                )
                .await?;

            all_locked_packages.extend(locked);
        }

        // Aggregate into workspace lockfile
        let workspace_lockfile = WorkspaceLockfile::try_from(all_locked_packages)?;

        // Write workspace lockfile
        workspace_lockfile.write(root_path).await?;

        tracing::info!(
            ":: wrote workspace lockfile at {}",
            root_path.join(LOCKFILE).display()
        );

        Ok(())
    }

    /// Installs dependencies of a package
    async fn install_package(
        &self,
        manifest: &PackagesManifest,
        lockfile: &PackageLockfile,
        store: &PackageStore,
        package_path: &PathBuf,
        workspace_mode: WorkspaceLockfileMode<'_>,
        write_mode: LockfileWriteMode,
    ) -> miette::Result<Vec<WorkspaceLockedPackage>> {
        store.clear().await?;

        if let Some(ref pkg) = manifest.package {
            store.populate(pkg).await?;

            tracing::info!(":: installed {}@{}", pkg.name, pkg.version);
        }

        let resolved_lockfile = match workspace_mode {
            WorkspaceLockfileMode::UseExisting(ws_lock) => {
                Some(Lockfile::Workspace(ws_lock.clone()))
            }
            WorkspaceLockfileMode::CreateNew => Some(Lockfile::Package(lockfile.clone())),
        };

        let graph =
            DependencyGraph::build(manifest, package_path, &self.credentials, resolved_lockfile)
                .await?;
        let dependencies = graph.ordered_dependencies()?;

        // 1. Install all dependencies and track resolved remote packages
        let mut resolved_remote_packages: HashMap<PackageName, ResolvedRemotePackage> =
            HashMap::new();

        for dependency in dependencies {
            // Iterate through the dependencies in order and install them
            let package = match dependency.node.source {
                DependencySource::Local { path } => self.install_local_dependency(&path).await?,
                DependencySource::Remote {
                    repository,
                    registry,
                } => {
                    let package_name = &dependency.node.name;
                    let version = &dependency.node.version;

                    let resolved_package = self
                        .install_remote_dependency(
                            package_name,
                            &registry,
                            &repository,
                            version,
                            workspace_mode,
                            lockfile,
                        )
                        .await?;

                    // Track this resolved remote package
                    resolved_remote_packages.insert(
                        package_name.clone(),
                        ResolvedRemotePackage {
                            package: resolved_package.clone(),
                            registry: registry.clone(),
                            repository: repository.clone(),
                        },
                    );

                    resolved_package
                }
            };

            store
                .unpack(&package)
                .await
                .wrap_err_with(|| format!("failed to unpack package {}", package.name()))?;

            tracing::info!(":: installed {}@{}", dependency.name, package.version());
        }

        // 2. Create LockedPackages with dependency information
        let mut locked_packages = Vec::new();

        for (pkg_name, resolved) in &resolved_remote_packages {
            let node = graph
                .nodes
                .get(pkg_name)
                .ok_or_else(|| miette::miette!("Package {} not found in graph", pkg_name))?;

            // Map dependency names to their resolved versions (only remote ones)
            let deps: Vec<LockedDependency> = node
                .dependencies
                .iter()
                .filter_map(|dep_name| {
                    resolved_remote_packages.get(dep_name).map(|dep_resolved| {
                        LockedDependency::new(
                            dep_name.clone(),
                            dep_resolved.package.version().clone(),
                        )
                    })
                })
                .collect();

            // For workspace lockfiles, set dependants=1 (this package needs it)
            // Aggregation will sum them up across all workspace members
            let dependants_count = 1;

            // Create WorkspaceLockedPackage with dependencies
            let workspace_locked = WorkspaceLockedPackage {
                name: resolved.package.name().clone(),
                version: resolved.package.version().clone(),
                digest: DigestAlgorithm::SHA256.digest(&resolved.package.tgz),
                registry: resolved.registry.clone(),
                repository: resolved.repository.clone(),
                dependencies: deps,
                dependants: dependants_count,
            };

            locked_packages.push(workspace_locked);
        }

        // Write lockfile based on mode (convert to package lockfile format)
        match write_mode {
            LockfileWriteMode::Write => {
                let package_lockfile: PackageLockfile = locked_packages.clone().try_into()?;
                package_lockfile.write(package_path).await?;
            }
            LockfileWriteMode::Skip => {
                // Workspace will handle lockfile writing
            }
        }

        Ok(locked_packages)
    }

    /// Installs a local dependency
    async fn install_local_dependency(&self, path: &Path) -> miette::Result<Package> {
        let dep_manifest =
            BuffrsManifest::require_package_manifest(&path.join(MANIFEST_FILE)).await?;
        let dep_store = PackageStore::open(path).await?;
        dep_store.release(&dep_manifest, self.preserve_mtime).await
    }

    /// Installs a remote dependency
    async fn install_remote_dependency(
        &self,
        package_name: &PackageName,
        registry: &RegistryUri,
        repository: &str,
        version: &VersionReq,
        workspace_mode: WorkspaceLockfileMode<'_>,
        lockfile: &PackageLockfile,
    ) -> miette::Result<Package> {
        // Prio 1: Check workspace lockfile
        if let WorkspaceLockfileMode::UseExisting(ws_lockfile) = workspace_mode
            && let Some(locked) =
                Self::find_matching_workspace_locked(ws_lockfile, package_name, version)
        {
            ensure!(
                registry == &locked.registry,
                "registry mismatch for {}: manifest specifies {} but workspace lockfile requires {}",
                package_name,
                registry,
                locked.registry
            );

            tracing::debug!(
                ":: using workspace lockfile version for {}@{}",
                package_name,
                locked.version
            );

            // Try to get from cache
            if let Some(cached_pkg) = self.cache.get(locked.into()).await? {
                locked.validate(&cached_pkg)?;
                return Ok(cached_pkg);
            }

            return self
                .download_exact_version(package_name, registry, repository, &locked.version)
                .await;
        }

        // Prio 2: Check package lockfile (for backward compatibility)
        if let Some(locked_entry) = lockfile.get(package_name) {
            ensure!(
                registry == &locked_entry.registry,
                "registry mismatch for {}: manifest specifies {} but lockfile requires {}",
                package_name,
                registry,
                locked_entry.registry
            );

            // Try to retrieve from cache if version matches lockfile
            if version.matches(&locked_entry.version)
                && let Some(cached_pkg) = self.cache.get(locked_entry.into()).await?
            {
                locked_entry.validate(&cached_pkg)?;

                tracing::debug!(
                    ":: using cached package for {}@{}",
                    package_name,
                    cached_pkg.version()
                );

                return Ok(cached_pkg);
            }
        }

        // Prio 3: Download from registry
        self.download_and_cache(package_name, registry, repository, version)
            .await
    }

    /// Finds a matching version in the workspace lockfile
    fn find_matching_workspace_locked<'a>(
        lockfile: &'a WorkspaceLockfile,
        name: &PackageName,
        requirement: &VersionReq,
    ) -> Option<&'a WorkspaceLockedPackage> {
        lockfile
            .packages()
            .filter(|pkg| pkg.name == *name)
            .filter(|pkg| requirement.matches(&pkg.version))
            .max_by_key(|pkg| &pkg.version) // Highest matching version (tiebreaker)
    }

    /// Downloads a specific exact version (used for workspace lockfile specifies it)
    async fn download_exact_version(
        &self,
        package_name: &PackageName,
        registry: &RegistryUri,
        repository: &str,
        version: &semver::Version,
    ) -> miette::Result<Package> {
        let version_req = VersionReq::parse(&format!("={}", version))
            .into_diagnostic()
            .wrap_err("failed to create exact version requirement")?;

        self.download_and_cache(package_name, registry, repository, &version_req)
            .await
    }

    /// Downloads a package from the registry and caches it
    async fn download_and_cache(
        &self,
        package_name: &PackageName,
        registry: &RegistryUri,
        repository: &str,
        version: &VersionReq,
    ) -> miette::Result<Package> {
        let artifactory = Artifactory::new(registry.clone(), &self.credentials)
            .wrap_err_with(|| format!("failed to initialize registry {}", registry))?;

        let dependency = Dependency {
            package: package_name.clone(),
            manifest: DependencyManifest::Remote(RemoteDependencyManifest {
                registry: registry.clone(),
                repository: repository.to_string(),
                version: version.clone(),
            }),
        };

        let downloaded_package = artifactory.download(dependency).await?;

        // Cache the downloaded package for future installs
        let cache_key = CacheEntry::from(&downloaded_package);
        self.cache
            .put(cache_key, downloaded_package.tgz.clone())
            .await
            .ok();

        Ok(downloaded_package)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lock::LockedPackage;
    use crate::manifest::GenericManifest;
    use crate::manifest::PackageManifest;
    use crate::package::PackageType;
    use semver::Version;
    use std::collections::HashMap;
    use std::str::FromStr;
    use tempfile::TempDir;
    use tokio::fs;

    // Helper to create test lockfile with a package
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
        let _temp_dir = TempDir::new().unwrap();
        let cache = Cache::open().await.unwrap();

        // Create lockfile with registry A
        let lockfile =
            create_lockfile_with_package("test-pkg", "1.0.0", "https://registry-a.com", "repo");

        let installer = Installer {
            preserve_mtime: false,
            credentials: Credentials {
                registry_tokens: HashMap::new(),
            },
            cache,
        };

        let pkg_name = PackageName::unchecked("test-pkg");
        // Try to install with registry B (different from lockfile)
        let registry_b = RegistryUri::from_str("https://registry-b.com").unwrap();
        let version = VersionReq::parse("1.0.0").unwrap();

        let result = installer
            .install_remote_dependency(
                &pkg_name,
                &registry_b,
                "repo",
                &version,
                WorkspaceLockfileMode::CreateNew,
                &lockfile,
            )
            .await;

        // Should fail with registry mismatch error
        assert!(result.is_err());
        let err_msg = format!("{:?}", result.unwrap_err());
        assert!(err_msg.contains("registry mismatch"));
        assert!(err_msg.contains("test-pkg"));
    }

    #[tokio::test]
    async fn test_version_mismatch_skips_cache() {
        let _temp_dir = TempDir::new().unwrap();
        let cache = Cache::open().await.unwrap();

        // Create lockfile with version 1.0.0
        let lockfile =
            create_lockfile_with_package("test-pkg", "1.0.0", "https://registry.com", "repo");

        let installer = Installer {
            preserve_mtime: false,
            credentials: Credentials {
                registry_tokens: HashMap::new(),
            },
            cache,
        };

        let pkg_name = PackageName::unchecked("test-pkg");
        let registry = RegistryUri::from_str("https://registry.com").unwrap();
        // Request version 2.0.0 (doesn't match lockfile's 1.0.0)
        let version = VersionReq::parse("2.0.0").unwrap();

        let result = installer
            .install_remote_dependency(
                &pkg_name,
                &registry,
                "repo",
                &version,
                WorkspaceLockfileMode::CreateNew,
                &lockfile,
            )
            .await;

        // Should try to download (will fail because no actual registry)
        // but the important thing is it doesn't try to use the cached version
        assert!(result.is_err());
        // Error should be about downloading, not about version mismatch validation
        let err_msg = format!("{:?}", result.unwrap_err());
        assert!(!err_msg.contains("registry mismatch"));
    }

    #[tokio::test]
    async fn test_missing_lockfile_entry_requires_download() {
        let _temp_dir = TempDir::new().unwrap();
        let cache = Cache::open().await.unwrap();

        // Empty lockfile
        let lockfile = PackageLockfile::default();

        let installer = Installer {
            preserve_mtime: false,
            credentials: Credentials {
                registry_tokens: HashMap::new(),
            },
            cache,
        };

        let pkg_name = PackageName::unchecked("new-pkg");
        let registry = RegistryUri::from_str("https://registry.com").unwrap();
        let version = VersionReq::parse("1.0.0").unwrap();

        let result = installer
            .install_remote_dependency(
                &pkg_name,
                &registry,
                "repo",
                &version,
                WorkspaceLockfileMode::CreateNew,
                &lockfile,
            )
            .await;

        // Should try to download (will fail because no actual registry)
        assert!(result.is_err());
        // Error should be about downloading
        let err_msg = format!("{:?}", result.unwrap_err());
        assert!(!err_msg.contains("registry mismatch"));
    }

    #[tokio::test]
    async fn test_install_local_dependency() {
        let temp_dir = TempDir::new().unwrap();
        let dep_dir = temp_dir.path().join("local-dep");
        fs::create_dir_all(&dep_dir).await.unwrap();

        // Create a minimal manifest for the local dependency
        let manifest = PackagesManifest::builder()
            .package(PackageManifest {
                kind: PackageType::Lib,
                name: PackageName::unchecked("local-lib"),
                version: Version::new(1, 0, 0),
                description: None,
            })
            .dependencies(Default::default())
            .build();

        manifest.write_at(&dep_dir).await.unwrap();

        // Create proto directory structure
        PackageStore::open(&dep_dir).await.unwrap();

        let cache = Cache::open().await.unwrap();
        let installer = Installer {
            preserve_mtime: false,
            credentials: Credentials {
                registry_tokens: HashMap::new(),
            },
            cache,
        };

        // Install the local dependency
        let result = installer.install_local_dependency(&dep_dir).await;

        assert!(result.is_ok());
        let package = result.unwrap();
        assert_eq!(package.name(), &PackageName::unchecked("local-lib"));
        assert_eq!(package.version(), &Version::new(1, 0, 0));
    }

    #[tokio::test]
    async fn test_install_local_dependency_preserve_mtime() {
        let temp_dir = TempDir::new().unwrap();
        let dep_dir = temp_dir.path().join("local-dep");
        fs::create_dir_all(&dep_dir).await.unwrap();

        let manifest = PackagesManifest::builder()
            .package(PackageManifest {
                kind: PackageType::Lib,
                name: PackageName::unchecked("local-lib"),
                version: Version::new(1, 0, 0),
                description: None,
            })
            .dependencies(Default::default())
            .build();

        manifest.write_at(&dep_dir).await.unwrap();
        PackageStore::open(&dep_dir).await.unwrap();

        let cache = Cache::open().await.unwrap();
        // Test with preserve_mtime = true
        let installer = Installer {
            preserve_mtime: true, // <-- Important
            credentials: Credentials {
                registry_tokens: HashMap::new(),
            },
            cache,
        };

        let result = installer.install_local_dependency(&dep_dir).await;

        // Should succeed (actual mtime preservation is tested in PackageStore tests)
        assert!(result.is_ok());
    }

    #[test]
    fn test_aggregate_workspace_lockfile_multiple_versions() {
        use crate::lock::{Digest, DigestAlgorithm, WorkspaceLockedPackage};

        // Create two different versions of the same package from different members
        let pkg_v1 = WorkspaceLockedPackage {
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

        let pkg_v2 = WorkspaceLockedPackage {
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
        let pkg_v1_dup = WorkspaceLockedPackage {
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

    #[test]
    fn test_find_matching_workspace_locked() {
        use crate::lock::{Digest, DigestAlgorithm, WorkspaceLockedPackage, WorkspaceLockfile};

        // Create workspace lockfile with multiple versions
        let pkg_v1 = WorkspaceLockedPackage {
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

        let pkg_v2 = WorkspaceLockedPackage {
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

        let lockfile = WorkspaceLockfile::from_iter(vec![pkg_v1, pkg_v2]);

        // Test finding version matching ^1.0.0
        let req_v1 = VersionReq::parse("^1.0.0").unwrap();
        let found = Installer::find_matching_workspace_locked(
            &lockfile,
            &PackageName::unchecked("remote-lib"),
            &req_v1,
        );
        assert!(found.is_some());
        assert_eq!(found.unwrap().version, Version::new(1, 5, 0));

        // Test finding version matching ^2.0.0
        let req_v2 = VersionReq::parse("^2.0.0").unwrap();
        let found = Installer::find_matching_workspace_locked(
            &lockfile,
            &PackageName::unchecked("remote-lib"),
            &req_v2,
        );
        assert!(found.is_some());
        assert_eq!(found.unwrap().version, Version::new(2, 0, 0));

        // Test not finding version matching ^3.0.0
        let req_v3 = VersionReq::parse("^3.0.0").unwrap();
        let found = Installer::find_matching_workspace_locked(
            &lockfile,
            &PackageName::unchecked("remote-lib"),
            &req_v3,
        );
        assert!(found.is_none());
    }
}
