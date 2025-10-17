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

use crate::manifest::{
    Dependency, DependencyManifest, LocalDependencyManifest, RemoteDependencyManifest, MANIFEST_FILE, Manifest,
};
use crate::package::PackageStore;
use crate::registry::{Artifactory, RegistryUri};
use miette::{Context as _, IntoDiagnostic, bail, miette};
use semver::VersionReq;
use std::collections::HashMap;
use std::path::Path;
use std::str::FromStr;

#[cfg(feature = "git")]
use std::process::Stdio;

/// Handles publishing of local packages to a registry
pub struct Publisher {
    registry: RegistryUri,
    repository: String,
    artifactory: Artifactory,
    preserve_mtime: bool,
    /// Mapping from local dependency paths to their remote published locations
    manifest_mappings: HashMap<LocalDependencyManifest, RemoteDependencyManifest>,
}

impl Publisher {
    /// Creates a new Publisher instance
    pub fn new(
        registry: RegistryUri,
        repository: String,
        artifactory: Artifactory,
        preserve_mtime: bool,
    ) -> Self {
        Self {
            registry,
            repository,
            artifactory,
            preserve_mtime,
            manifest_mappings: HashMap::new(),
        }
    }

    /// Checks git status and ensures repository is clean before publishing
    ///
    /// Returns an error if the repository has uncommitted changes and `allow_dirty` is false.
    #[cfg(feature = "git")]
    pub async fn check_git_status(allow_dirty: bool) -> miette::Result<()> {
        let statuses = Self::get_git_statuses().await?;

        if !allow_dirty && !statuses.is_empty() {
            tracing::error!(
                "{} files in the working directory contain changes that were not yet committed into git:\n",
                statuses.len()
            );

            statuses.iter().for_each(|s| tracing::error!("{}", s));

            tracing::error!(
                "\nTo proceed with publishing despite the uncommitted changes, pass the `--allow-dirty` flag\n"
            );

            bail!("attempted to publish a dirty repository");
        }

        Ok(())
    }

    /// Gets the list of files with uncommitted changes from git
    #[cfg(feature = "git")]
    async fn get_git_statuses() -> miette::Result<Vec<String>> {
        let output = tokio::process::Command::new("git")
            .arg("status")
            .arg("--porcelain")
            .stderr(Stdio::null())
            .output()
            .await;

        let Ok(output) = output else {
            return Ok(Vec::new());
        };

        if !output.status.success() {
            return Ok(Vec::new());
        }

        let stdout = String::from_utf8(output.stdout)
            .into_diagnostic()
            .wrap_err(miette!(
                "invalid utf-8 character in the output of `git status`"
            ))?;

        let lines: Option<Vec<_>> = stdout
            .lines()
            .map(|line| {
                line.split_once(' ')
                    .map(|(_, filename)| filename.to_string())
            })
            .collect();

        Ok(lines.unwrap_or_default())
    }

    /// Publishes a local package at the given path
    ///
    /// This method:
    /// 1. Reads the package manifest
    /// 2. Replaces local dependencies with their published remote versions
    /// 3. Creates a package with the updated manifest
    /// 4. Publishes to the registry
    /// 5. Records the mapping of local path to remote location
    pub async fn publish_package(&mut self, package_path: &Path) -> miette::Result<()> {
        let manifest_path = package_path.join(MANIFEST_FILE);
        let manifest = Manifest::try_read_from(&manifest_path)
            .await
            .wrap_err(miette!(
                "Failed to read manifest file at {}",
                package_path.display()
            ))?;

        // Create a store at the package's path
        let package_store = PackageStore::open(package_path).await?;

        let remote_dependencies = self.replace_local_with_remote_dependencies(&manifest, package_path)?;

        // Clone manifest with local dependencies replaced by their remote locations
        let remote_deps_manifest = manifest.clone_with_different_dependencies(remote_dependencies);
        let package = package_store
            .release(&remote_deps_manifest, self.preserve_mtime)
            .await?;

        self.artifactory
            .publish(package.clone(), self.repository.clone())
            .await
            .wrap_err(miette!("publishing of package {} failed", package.name()))?;

        // Store the mapping for this package
        let local_manifest = LocalDependencyManifest {
            path: manifest_path,
        };

        let package_version = VersionReq::from_str(package.version().to_string().as_str())
            .into_diagnostic()?;

        let remote_manifest = RemoteDependencyManifest {
            version: package_version,
            registry: self.registry.clone(),
            repository: self.repository.clone(),
        };

        self.manifest_mappings.insert(local_manifest, remote_manifest);

        Ok(())
    }

    /// Replaces local dependencies in a manifest with their published remote versions
    fn replace_local_with_remote_dependencies(
        &self,
        manifest: &Manifest,
        base_path: &Path,
    ) -> miette::Result<Vec<Dependency>> {
        // Manifest may contain references to other local dependencies that need to be replaced by their remote locations
        // The topological order of `ordered_dependencies` guarantees that all dependant packages have been published at this point
        // Keep remote dependencies
        let mut remote_dependencies: Vec<Dependency> = manifest.get_remote_dependencies();
        let local_dependencies: Vec<Dependency> = manifest.get_local_dependencies();

        // Replace all local dependencies with the corresponding remote manifests created as part of their own processing
        for local_dep in local_dependencies {
            match local_dep.manifest {
                DependencyManifest::Local(local_manifest) => {
                    // Paths in the manifest are relative and need to be converted to absolute paths to be used as unique keys
                    let absolute_path_manifest = LocalDependencyManifest {
                        path: base_path.join(&local_manifest.path).join(MANIFEST_FILE),
                    };

                    let remote_manifest = self.manifest_mappings
                        .get(&absolute_path_manifest)
                        .wrap_err(miette!("local dependency {} should have been made available during publish, but is not found",
                        &local_dep.package))?;

                    let remote_dependency = Dependency {
                        package: local_dep.package.clone(),
                        manifest: DependencyManifest::Remote(remote_manifest.clone()),
                    };
                    remote_dependencies.push(remote_dependency)
                }
                _ => bail!("remote dependency manifest found at an unexpected place"),
            }
        }
        Ok(remote_dependencies)
    }
}
