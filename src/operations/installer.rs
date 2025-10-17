// Copyright 2023 Helsing GmbH
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use crate::cache::{Cache, Entry as CacheEntry};
use crate::credentials::Credentials;
use crate::lock::{Lockfile, LOCKFILE};
use crate::manifest::{
    Dependency, DependencyManifest, MANIFEST_FILE, Manifest, ManifestType,
    RemoteDependencyManifest,
};
use crate::package::{Package, PackageStore};
use crate::registry::{Artifactory, RegistryUri};
use crate::resolver::{DependencyGraph, DependencySource};
use miette::{Context as _, IntoDiagnostic, ensure, miette};
use semver::VersionReq;
use std::env;
use std::path::{Path, PathBuf};
use tokio::fs;

use crate::package::PackageName;

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
    pub async fn install(&self, manifest: &Manifest) -> miette::Result<()> {
        match manifest.manifest_type {
            ManifestType::Package => {
                let lockfile = Lockfile::read_or_default().await?;
                let store = PackageStore::current().await?;
                let current_path = env::current_dir()
                    .into_diagnostic()
                    .wrap_err("current dir could not be retrieved")?;

                self.install_package(manifest, &lockfile, &store, &current_path)
                    .await
            }
            ManifestType::Workspace => self.install_workspace(manifest).await,
        }
    }

    /// Installs dependencies for a workspace
    async fn install_workspace(&self, manifest: &Manifest) -> miette::Result<()> {
        let root_path = env::current_dir()
            .into_diagnostic()
            .wrap_err("current dir could not be retrieved")?;
        let workspace = manifest.workspace.as_ref().ok_or_else(|| {
            miette!(
                "buffers install for workspaces executed on manifest that does not define a workspace."
            )
        })?;
        let packages = workspace.resolve_members(root_path)?;
        tracing::info!(
            ":: workspace found. running install for {} packages in workspace",
            packages.len()
        );

        for package in packages {
            let canonical_name = fs::canonicalize(&package).await.into_diagnostic()?;
            let pkg_manifest = Manifest::try_read_from(package.join(MANIFEST_FILE)).await?;
            let pkg_lockfile = Lockfile::read_from_or_default(package.join(LOCKFILE)).await?;
            let store = PackageStore::open(&package).await?;

            tracing::info!(
                ":: running install for package: {}",
                canonical_name.to_str().unwrap()
            );
            self.install_package(&pkg_manifest, &pkg_lockfile, &store, &package)
                .await?
        }

        Ok(())
    }

    /// Installs dependencies of a package
    async fn install_package(
        &self,
        manifest: &Manifest,
        lockfile: &Lockfile,
        store: &PackageStore,
        package_path: &PathBuf,
    ) -> miette::Result<()> {
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
                DependencySource::Local { path } => {
                    self.install_local_dependency(&path).await?
                }
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
                .wrap_err(miette!("failed to unpack package {}", &package.name()))?;

            tracing::info!(
                ":: installed {}@{}",
                dependency_node.name,
                package.version()
            );
        }

        // Write lockfile
        Lockfile::from_iter(locked.into_iter())
            .write(package_path)
            .await
    }

    /// Installs a local dependency
    async fn install_local_dependency(&self, path: &Path) -> miette::Result<Package> {
        let dep_manifest = Manifest::try_read_from(path.join(MANIFEST_FILE)).await?;
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
            if version.matches(&locked_entry.version) {
                if let Some(cached_pkg) = self.cache.get(locked_entry.into()).await? {
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
            .wrap_err(miette!("failed to initialize registry {}", registry))?;

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
