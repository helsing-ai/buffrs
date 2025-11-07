// (c) Copyright 2025 Helsing GmbH. All rights reserved.

use std::{
    env,
    path::{Path, PathBuf},
};

use miette::{Context as _, IntoDiagnostic, ensure};
use semver::VersionReq;

use crate::{
    cache::{Cache, Entry as CacheEntry},
    credentials::Credentials,
    lock::{LOCKFILE, LockedPackage, Lockfile, WorkspaceLockedPackage, WorkspaceLockfile},
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
                let lockfile = Lockfile::read_or_default().await?;
                let store = PackageStore::current().await?;
                let current_path = env::current_dir()
                    .into_diagnostic()
                    .wrap_err("current dir could not be retrieved")?;

                self.install_package(
                    packages_manifest,
                    &lockfile,
                    &store,
                    &current_path,
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

    /// Installs dependencies for a workspace
    async fn install_workspace(&self, manifest: &WorkspaceManifest) -> miette::Result<()> {
        let root_path = env::current_dir()
            .into_diagnostic()
            .wrap_err("current dir could not be retrieved")?;

        let packages = manifest.workspace.resolve_members(&root_path)?;
        tracing::info!(
            ":: workspace found. running install for {} packages in workspace",
            packages.len()
        );

        let mut all_locked_packages = Vec::new();

        for package in packages {
            let pkg_manifest =
                BuffrsManifest::require_package_manifest(&package.join(MANIFEST_FILE)).await?;

            // Warn if package lockfile exists
            let pkg_lockfile_path = package.join(LOCKFILE);
            if Lockfile::exists_at(&pkg_lockfile_path).await? {
                tracing::warn!(
                    ":: package lockfile found at {}. Consider removing it - workspace installs use workspace-level lockfile",
                    pkg_lockfile_path.display()
                );
            }

            let pkg_lockfile = Lockfile::read_from_or_default(&pkg_lockfile_path).await?;
            let store = PackageStore::open(&package).await?;

            tracing::info!(":: running install for package: {}", package.display());

            // Install without writing package lockfile (workspace will write workspace lockfile)
            let locked = self
                .install_package(
                    &pkg_manifest,
                    &pkg_lockfile,
                    &store,
                    &package,
                    LockfileWriteMode::Skip,
                )
                .await?;

            all_locked_packages.extend(locked);
        }

        // Aggregate into workspace lockfile
        let workspace_lockfile = Self::aggregate_workspace_lockfile(all_locked_packages)?;

        // Write workspace lockfile
        workspace_lockfile.write(&root_path).await?;

        tracing::info!(":: wrote workspace lockfile at {}", root_path.join(LOCKFILE).display());

        Ok(())
    }

    /// Aggregates package lockfiles into a workspace lockfile
    ///
    /// Merges locked packages from multiple members, deduplicating by (name, version)
    /// and summing dependants counts.
    fn aggregate_workspace_lockfile(
        locked_packages: Vec<LockedPackage>,
    ) -> miette::Result<WorkspaceLockfile> {
        use std::collections::HashMap;

        let mut workspace_packages: HashMap<(PackageName, semver::Version), WorkspaceLockedPackage> =
            HashMap::new();

        for locked in locked_packages {
            let key = (locked.name.clone(), locked.version.clone());

            workspace_packages
                .entry(key)
                .and_modify(|existing| {
                    // Same package (name, version) - sum dependants
                    existing.dependants += locked.dependants;

                    // Verify consistency of other fields
                    if existing.registry != locked.registry {
                        tracing::warn!(
                            "registry mismatch for {}@{}: {} vs {}. Using first seen.",
                            locked.name,
                            locked.version,
                            existing.registry,
                            locked.registry
                        );
                    }
                    if existing.digest != locked.digest {
                        tracing::warn!(
                            "digest mismatch for {}@{}: {} vs {}. Using first seen.",
                            locked.name,
                            locked.version,
                            existing.digest,
                            locked.digest
                        );
                    }
                })
                .or_insert_with(|| WorkspaceLockedPackage {
                    name: locked.name,
                    version: locked.version,
                    registry: locked.registry,
                    repository: locked.repository,
                    digest: locked.digest,
                    dependencies: vec![], // Empty for now - will add dependency graph capture later
                    dependants: locked.dependants,
                });
        }

        Ok(WorkspaceLockfile::from_iter(
            workspace_packages.into_values(),
        ))
    }

    /// Installs dependencies of a package
    ///
    /// Returns the lockfile data. Lockfile persistence depends on `write_mode`.
    async fn install_package(
        &self,
        manifest: &PackagesManifest,
        lockfile: &Lockfile,
        store: &PackageStore,
        package_path: &PathBuf,
        write_mode: LockfileWriteMode,
    ) -> miette::Result<Vec<LockedPackage>> {
        store.clear().await?;

        if let Some(ref pkg) = manifest.package {
            store.populate(pkg).await?;

            tracing::info!(":: installed {}@{}", pkg.name, pkg.version);
        }

        let graph = DependencyGraph::build(manifest, package_path, &self.credentials).await?;
        let dependencies = graph.ordered_dependencies()?;

        let mut locked = Vec::new();

        for dependency_node in dependencies {
            // Iterate through the dependencies in order and install them
            let package = match dependency_node.node.source {
                DependencySource::Local { path } => self.install_local_dependency(&path).await?,
                DependencySource::Remote {
                    repository,
                    registry,
                } => {
                    let package_name = &dependency_node.node.name;
                    let version = &dependency_node.node.version;

                    let resolved_package = self
                        .install_remote_dependency(
                            package_name,
                            &registry,
                            &repository,
                            version,
                            lockfile,
                        )
                        .await?;

                    // Add to new lockfile
                    let dependants_count = graph.dependants_count_of(package_name);
                    locked.push(resolved_package.lock(registry, repository, dependants_count));

                    resolved_package
                }
            };

            store
                .unpack(&package)
                .await
                .wrap_err_with(|| format!("failed to unpack package {}", package.name()))?;

            tracing::info!(
                ":: installed {}@{}",
                dependency_node.name,
                package.version()
            );
        }

        // Write lockfile based on mode
        match write_mode {
            LockfileWriteMode::Write => {
                Lockfile::from_iter(locked.iter().cloned())
                    .write(package_path)
                    .await?;
            }
            LockfileWriteMode::Skip => {
                // Workspace will handle lockfile writing
            }
        }

        Ok(locked)
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
        lockfile: &Lockfile,
    ) -> miette::Result<Package> {
        // Try to use cached package if available in lockfile
        let mut resolved_package = None;
        if let Some(locked_entry) = lockfile.get(package_name) {
            // Validate registry consistency
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
                // Validate the cached package digest
                locked_entry.validate(&cached_pkg)?;

                tracing::debug!(
                    ":: using cached package for {}@{}",
                    package_name,
                    cached_pkg.version()
                );

                resolved_package = Some(cached_pkg);
            }
        }

        // Download from registry if not cached
        match resolved_package {
            Some(pkg) => Ok(pkg),
            None => {
                self.download_and_cache(package_name, registry, repository, version)
                    .await
            }
        }
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
    ) -> Lockfile {
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
        Lockfile::from_iter(vec![locked_pkg])
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
            .install_remote_dependency(&pkg_name, &registry_b, "repo", &version, &lockfile)
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
            .install_remote_dependency(&pkg_name, &registry, "repo", &version, &lockfile)
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
        let lockfile = Lockfile::default();

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
            .install_remote_dependency(&pkg_name, &registry, "repo", &version, &lockfile)
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
        use crate::lock::{Digest, DigestAlgorithm};

        // Create two different versions of the same package from different members
        let pkg_v1 = LockedPackage {
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

        let pkg_v2 = LockedPackage {
            name: PackageName::unchecked("remote-lib"),
            version: Version::new(2, 0, 0),
            registry: RegistryUri::from_str("https://registry.com").unwrap(),
            repository: "test-repo".to_string(),
            digest: Digest::from_parts(
                DigestAlgorithm::SHA256,
                "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            )
            .unwrap(),
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

        let workspace_lockfile = Installer::aggregate_workspace_lockfile(locked_packages).unwrap();

        // Should have 2 entries (v1.0.0 and v2.0.0), not 3
        assert_eq!(workspace_lockfile.packages().count(), 2);

        // v1.0.0 should have dependants=2 (merged from two members)
        let v1 = workspace_lockfile
            .get(&PackageName::unchecked("remote-lib"), &Version::new(1, 0, 0))
            .expect("v1.0.0 should exist");
        assert_eq!(v1.version, Version::new(1, 0, 0));
        assert_eq!(v1.dependants, 2); // Summed from two members

        // v2.0.0 should have dependants=1
        let v2 = workspace_lockfile
            .get(&PackageName::unchecked("remote-lib"), &Version::new(2, 0, 0))
            .expect("v2.0.0 should exist");
        assert_eq!(v2.version, Version::new(2, 0, 0));
        assert_eq!(v2.dependants, 1);
    }
}
