// (c) Copyright 2025 Helsing GmbH. All rights reserved.

use std::collections::HashMap;
use std::env;
use std::path::Path;
#[cfg(feature = "git")]
use std::process::Stdio;
use std::str::FromStr;

use miette::{Context as _, IntoDiagnostic, bail, miette};
use semver::{Version, VersionReq};

use crate::{
    credentials::Credentials,
    manifest::{
        BuffrsManifest, Dependency, DependencyManifest, LocalDependencyManifest, MANIFEST_FILE,
        PackagesManifest, RemoteDependencyManifest, WorkspaceManifest,
    },
    package::PackageStore,
    registry::{Artifactory, RegistryUri},
    resolver::{DependencyGraph, DependencySource},
};

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
    pub async fn new(
        registry: RegistryUri,
        repository: String,
        preserve_mtime: bool,
    ) -> miette::Result<Self> {
        tracing::debug!("Publisher::new() called");
        tracing::debug!("  registry: {}", registry);
        tracing::debug!("  repository: {}", repository);
        tracing::debug!("  preserve_mtime: {}", preserve_mtime);

        tracing::debug!("loading credentials for publisher");
        let credentials = Credentials::load().await?;
        tracing::debug!("credentials loaded successfully");

        tracing::debug!("creating artifactory client for registry: {}", registry);
        let artifactory = Artifactory::new(registry.clone(), &credentials)?;
        tracing::debug!("artifactory client created successfully");

        tracing::debug!("publisher instance created successfully");
        Ok(Self {
            registry,
            repository,
            artifactory,
            preserve_mtime,
            manifest_mappings: HashMap::new(),
        })
    }

    /// Checks git status and ensures repository is clean before publishing
    ///
    /// Returns an error if the repository has uncommitted changes and `allow_dirty` is false.
    #[cfg(feature = "git")]
    pub async fn check_git_status(allow_dirty: bool) -> miette::Result<()> {
        tracing::debug!("check_git_status() called");
        tracing::debug!("  allow_dirty: {}", allow_dirty);

        tracing::debug!("retrieving uncommitted files from git");
        let statuses = Self::get_uncommitted_files().await?;
        tracing::debug!("found {} uncommitted files", statuses.len());

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

        tracing::debug!("git status check passed");
        Ok(())
    }

    /// Gets the list of files with uncommitted changes from git
    #[cfg(feature = "git")]
    async fn get_uncommitted_files() -> miette::Result<Vec<String>> {
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
            .wrap_err("invalid utf-8 character in the output of `git status`")?;

        let lines: Option<Vec<_>> = stdout
            .lines()
            .map(|line| {
                line.split_once(' ')
                    .map(|(_, filename)| filename.to_string())
            })
            .collect();

        Ok(lines.unwrap_or_default())
    }

    /// Main entry point for publishing
    ///
    /// Dispatches to either package or workspace publishing based on manifest type
    pub async fn publish(
        &mut self,
        manifest: &BuffrsManifest,
        package_path: &Path,
        version: Option<Version>,
        dry_run: bool,
    ) -> miette::Result<()> {
        tracing::debug!("Publisher::publish() called");
        tracing::debug!("  package_path: {}", package_path.display());
        tracing::debug!("  version: {:?}", version);
        tracing::debug!("  dry_run: {}", dry_run);
        tracing::debug!("  registry: {}", self.registry);
        tracing::debug!("  repository: {}", self.repository);

        if dry_run {
            tracing::warn!(":: aborting upload due to dry run");
            return Ok(());
        }

        match manifest {
            BuffrsManifest::Package(packages_manifest) => {
                tracing::debug!("manifest type: Package");
                if let Some(ref pkg) = packages_manifest.package {
                    tracing::debug!("  package name: {}", pkg.name);
                    tracing::debug!("  package version: {}", pkg.version);
                }
                self.publish_package_from_manifest(packages_manifest, package_path, version)
                    .await
            }
            BuffrsManifest::Workspace(workspace_manifest) => {
                tracing::debug!("manifest type: Workspace");
                if version.is_some() {
                    bail!(":: version flag is not supported for workspace publishing");
                }
                self.publish_workspace_from_manifest(workspace_manifest)
                    .await
            }
        }
    }

    /// Publishes a single package from its manifest
    async fn publish_package_from_manifest(
        &mut self,
        manifest: &PackagesManifest,
        package_path: &Path,
        version: Option<Version>,
    ) -> miette::Result<()> {
        tracing::debug!("publish_package_from_manifest() called");
        tracing::debug!("  package_path: {}", package_path.display());
        tracing::debug!("  version override: {:?}", version);

        if let Some(ref pkg) = manifest.package {
            tracing::debug!("  manifest package name: {}", pkg.name);
            tracing::debug!("  manifest package version: {}", pkg.version);
            tracing::debug!("  manifest package kind: {:?}", pkg.kind);
        }

        let mut root_manifest = manifest.clone();

        tracing::debug!("opening package store at current directory");
        let store = PackageStore::current().await?;
        tracing::debug!("package store opened successfully");

        if let Some(version) = version
            && let Some(ref mut package) = root_manifest.package
        {
            tracing::info!(":: modified version in published manifest to {version}");
            tracing::debug!("manifest mutation: overriding version {} -> {}", package.version, version);
            tracing::debug!("  package: {}", package.name);
            tracing::debug!("  old version: {}", package.version);
            tracing::debug!("  new version: {}", version);
            package.version = version;
        }

        // Build dependency graph
        tracing::debug!("building dependency graph for package at {}", package_path.display());
        let credentials = Credentials::load().await?;
        tracing::debug!("credentials loaded for dependency graph building");

        let graph =
            DependencyGraph::build(&root_manifest, package_path, &credentials, None).await?;
        tracing::debug!("dependency graph built successfully");

        let ordered_dependencies = graph.ordered_dependencies()?;
        tracing::debug!("dependency graph has {} total dependencies", ordered_dependencies.len());

        // Publish local dependencies first
        let local_deps: Vec<_> = ordered_dependencies
            .iter()
            .filter(|d| matches!(d.node.source, DependencySource::Local { .. }))
            .collect();

        tracing::debug!("found {} local dependencies to publish recursively", local_deps.len());
        if !local_deps.is_empty() {
            for (idx, dep) in local_deps.iter().enumerate() {
                tracing::debug!("  local dependency {}/{}: {}", idx + 1, local_deps.len(), dep.node.name);
            }
        }

        for (idx, dependency) in ordered_dependencies.iter().enumerate() {
            tracing::debug!("processing dependency {}/{}: {}", idx + 1, ordered_dependencies.len(), dependency.node.name);
            if let DependencySource::Local {
                path: absolute_path,
            } = &dependency.node.source
            {
                tracing::warn!(":: recursively publishing local dependency: {}", absolute_path.display());
                tracing::debug!("  dependency name: {}", dependency.node.name);
                tracing::debug!("  dependency path: {}", absolute_path.display());
                self.publish_package_at_path(&absolute_path, None).await?;
                tracing::debug!("local dependency {} published successfully", dependency.node.name);
            }
        }

        // Populate and publish the root package
        if let Some(ref pkg) = root_manifest.package {
            tracing::debug!("populating package store for package: {}", pkg.name);
            tracing::debug!("  package version: {}", pkg.version);
            tracing::debug!("  package kind: {:?}", pkg.kind);
            store.populate(pkg).await?;
            tracing::debug!("package store populated successfully for {}", pkg.name);
        }

        tracing::debug!("publishing root package at path: {}", package_path.display());
        tracing::debug!("passing modified root_manifest with potentially overridden version");
        self.publish_package_at_path(package_path, Some(&root_manifest)).await?;
        tracing::debug!("root package published successfully");

        Ok(())
    }

    /// Publishes all packages in a workspace
    async fn publish_workspace_from_manifest(
        &mut self,
        manifest: &WorkspaceManifest,
    ) -> miette::Result<()> {
        tracing::debug!("publish_workspace_from_manifest() called");

        let root_path = env::current_dir()
            .into_diagnostic()
            .wrap_err("current dir could not be retrieved")?;
        tracing::debug!("  workspace root path: {}", root_path.display());

        let packages = manifest.workspace.resolve_members(root_path)?;
        tracing::debug!("  resolved {} workspace members", packages.len());

        tracing::info!(
            ":: workspace found. publishing {} packages in workspace",
            packages.len()
        );
        tracing::debug!("workspace members: {:?}", packages.iter().map(|p| p.display().to_string()).collect::<Vec<_>>());

        tracing::debug!("loading credentials for workspace publishing");
        let credentials = Credentials::load().await?;
        tracing::debug!("credentials loaded successfully");

        // Iterate through each workspace member
        for (idx, member_path) in packages.iter().enumerate() {
            tracing::info!(":: processing workspace member {}/{}: {}", idx + 1, packages.len(), member_path.display());
            tracing::debug!("  member path: {}", member_path.display());

            let manifest_file = member_path.join(MANIFEST_FILE);
            tracing::debug!("IO: reading manifest from {}", manifest_file.display());
            let member_manifest =
                BuffrsManifest::require_package_manifest(&manifest_file).await?;
            tracing::debug!("manifest loaded successfully");

            if let Some(ref pkg) = member_manifest.package {
                tracing::debug!("  workspace member package name: {}", pkg.name);
                tracing::debug!("  workspace member package version: {}", pkg.version);
                tracing::debug!("  workspace member package kind: {:?}", pkg.kind);
            }

            // Build dependency graph for this member
            tracing::debug!("building dependency graph for workspace member: {}", member_path.display());
            let graph =
                DependencyGraph::build(&member_manifest, member_path, &credentials, None).await?;
            tracing::debug!("dependency graph built successfully");

            let dependencies = graph.ordered_dependencies()?;
            tracing::debug!("workspace member has {} total dependencies", dependencies.len());

            // Publish local dependencies first
            let local_deps: Vec<_> = dependencies
                .iter()
                .filter(|d| matches!(d.node.source, DependencySource::Local { .. }))
                .collect();

            tracing::debug!("workspace member has {} local dependencies to publish", local_deps.len());
            if !local_deps.is_empty() {
                for (dep_idx, dep) in local_deps.iter().enumerate() {
                    tracing::debug!("  local dependency {}/{}: {}", dep_idx + 1, local_deps.len(), dep.node.name);
                }
            }

            for (dep_idx, dependency) in dependencies.iter().enumerate() {
                tracing::debug!("processing workspace member dependency {}/{}: {}", dep_idx + 1, dependencies.len(), dependency.node.name);
                if let DependencySource::Local {
                    path: absolute_path,
                } = &dependency.node.source
                {
                    tracing::warn!(":: recursively publishing local dependency from workspace: {}", absolute_path.display());
                    tracing::debug!("  dependency name: {}", dependency.node.name);
                    tracing::debug!("  dependency path: {}", absolute_path.display());
                    self.publish_package_at_path(&absolute_path, None).await?;
                    tracing::debug!("local dependency {} published successfully", dependency.node.name);
                }
            }

            // Populate and publish the member itself
            if let Some(ref pkg) = member_manifest.package {
                tracing::debug!("populating workspace member package store for {}", pkg.name);
                tracing::debug!("  package version: {}", pkg.version);
                let member_store = PackageStore::open(member_path).await?;
                tracing::debug!("workspace member store opened at: {}", member_path.display());
                member_store.populate(pkg).await?;
                tracing::debug!("workspace member package store populated successfully for {}", pkg.name);
            }

            tracing::debug!("publishing workspace member at path: {}", member_path.display());
            self.publish_package_at_path(member_path, None).await?;
            tracing::debug!("workspace member published successfully");
        }

        tracing::debug!("all workspace members published successfully");
        Ok(())
    }

    /// Publishes a local package at the given path
    ///
    /// This method:
    /// 1. Checks if already published (idempotent)
    /// 2. Reads the package manifest (or uses provided manifest)
    /// 3. Replaces local dependencies with their published remote versions
    /// 4. Creates a package with the updated manifest
    /// 5. Publishes to the registry
    /// 6. Records the mapping of local path to remote location
    async fn publish_package_at_path(
        &mut self,
        package_path: &Path,
        manifest_override: Option<&PackagesManifest>,
    ) -> miette::Result<()> {
        tracing::debug!("publish_package_at_path() called");
        tracing::debug!("  package_path: {}", package_path.display());
        tracing::debug!("  manifest_override provided: {}", manifest_override.is_some());

        let manifest_path = package_path.join(MANIFEST_FILE);
        tracing::debug!("  manifest_path: {}", manifest_path.display());

        // Check if this package has already been published (idempotent)
        let local_manifest = LocalDependencyManifest {
            path: manifest_path.clone(),
        };

        tracing::debug!("checking if package already published (idempotency check)");
        tracing::debug!("  current manifest_mappings count: {}", self.manifest_mappings.len());
        if self.manifest_mappings.contains_key(&local_manifest) {
            tracing::debug!(
                ":: package at {} already published in this session, skipping to avoid duplicate publish",
                package_path.display()
            );
            return Ok(());
        }
        tracing::debug!("package not yet published in this session, proceeding");

        let manifest = if let Some(manifest_override) = manifest_override {
            tracing::debug!("using provided manifest override instead of reading from disk");
            manifest_override.clone()
        } else {
            tracing::debug!("IO: reading manifest from {}", manifest_path.display());
            BuffrsManifest::require_package_manifest(&manifest_path)
                .await
                .wrap_err_with(|| {
                    format!("failed to read manifest file at {}", package_path.display())
                })?
        };
        tracing::debug!("manifest obtained successfully");

        if let Some(ref pkg) = manifest.package {
            tracing::debug!("  package name: {}", pkg.name);
            tracing::debug!("  package version: {}", pkg.version);
            tracing::debug!("  package kind: {:?}", pkg.kind);
        }

        // Create a store at the package's path
        tracing::debug!("opening package store at: {}", package_path.display());
        let package_store = PackageStore::open(package_path).await?;
        tracing::debug!("package store opened successfully");

        let local_deps_count = manifest.get_local_dependencies().len();
        let remote_deps_count = manifest.get_remote_dependencies().len();
        tracing::debug!("manifest has {} local dependencies and {} remote dependencies", local_deps_count, remote_deps_count);

        if local_deps_count > 0 {
            tracing::debug!("manifest mutation: replacing {} local dependencies with remote versions", local_deps_count);
            let local_deps = manifest.get_local_dependencies();
            for (idx, dep) in local_deps.iter().enumerate() {
                tracing::debug!("  local dependency {}/{}: {}", idx + 1, local_deps_count, dep.package);
            }
        }

        let remote_dependencies =
            self.replace_local_with_remote_dependencies(&manifest, package_path)?;
        tracing::debug!("local dependencies replaced successfully, now have {} total remote dependencies", remote_dependencies.len());

        // Clone manifest with local dependencies replaced by their remote locations
        tracing::debug!("manifest mutation: creating manifest with {} remote dependencies", remote_dependencies.len());
        let remote_deps_manifest = manifest.with_dependencies(remote_dependencies);

        tracing::debug!("creating release package from store");
        tracing::debug!("  preserve_mtime: {}", self.preserve_mtime);
        let package = package_store
            .release(&remote_deps_manifest, self.preserve_mtime)
            .await?;
        tracing::debug!("release package created successfully");

        tracing::debug!("uploading package to registry");
        tracing::debug!("  package name: {}", package.name());
        tracing::debug!("  package version: {}", package.version());
        tracing::debug!("  registry: {}", self.registry);
        tracing::debug!("  repository: {}", self.repository);
        tracing::info!(":: uploading {} v{} to {}:{}", package.name(), package.version(), self.registry, self.repository);

        self.artifactory
            .publish(package.clone(), self.repository.clone())
            .await
            .wrap_err_with(|| format!("publishing of package {} failed", package.name()))?;

        tracing::info!(":: upload complete: {} v{}", package.name(), package.version());
        tracing::debug!("package uploaded successfully to registry");

        // Store the mapping for this package
        let package_version =
            VersionReq::from_str(&package.version().to_string()).into_diagnostic()?;
        tracing::debug!("converted package version to version requirement: {}", package_version);

        let remote_manifest = RemoteDependencyManifest {
            version: package_version.clone(),
            registry: self.registry.clone(),
            repository: self.repository.clone(),
        };

        tracing::debug!(
            "recording manifest mapping: {} -> {}:{}@{}",
            manifest_path.display(),
            self.registry,
            self.repository,
            package_version
        );

        self.manifest_mappings
            .insert(local_manifest, remote_manifest);
        tracing::debug!("manifest mapping recorded, total mappings: {}", self.manifest_mappings.len());

        Ok(())
    }

    /// Replaces local dependencies in a manifest with their published remote versions
    fn replace_local_with_remote_dependencies(
        &self,
        manifest: &PackagesManifest,
        base_path: &Path,
    ) -> miette::Result<Vec<Dependency>> {
        tracing::debug!("replace_local_with_remote_dependencies() called");
        tracing::debug!("  base_path: {}", base_path.display());

        // Manifest may contain references to other local dependencies that need to be replaced by their remote locations
        // The topological order of `ordered_dependencies` guarantees that all dependant packages have been published at this point
        // Keep remote dependencies
        let mut remote_dependencies: Vec<Dependency> = manifest.get_remote_dependencies();
        let local_dependencies: Vec<Dependency> = manifest.get_local_dependencies();

        tracing::debug!(
            "replacing local dependencies: {} local, {} existing remote",
            local_dependencies.len(),
            remote_dependencies.len()
        );

        // Replace all local dependencies with the corresponding remote manifests created as part of their own processing
        for (idx, local_dep) in local_dependencies.iter().enumerate() {
            tracing::debug!("processing local dependency {}/{}: {}", idx + 1, local_dependencies.len(), local_dep.package);

            match &local_dep.manifest {
                DependencyManifest::Local(local_manifest) => {
                    tracing::debug!("  local path in manifest: {}", local_manifest.path.display());

                    // Paths in the manifest are relative and need to be converted to absolute paths to be used as unique keys
                    let absolute_path_manifest = LocalDependencyManifest {
                        path: base_path.join(&local_manifest.path).join(MANIFEST_FILE),
                    };
                    tracing::debug!("  absolute path: {}", absolute_path_manifest.path.display());

                    tracing::debug!("looking up remote mapping for local dependency: {}", local_dep.package);
                    tracing::debug!("  available mappings: {}", self.manifest_mappings.len());

                    let remote_manifest = self
                        .manifest_mappings
                        .get(&absolute_path_manifest)
                        .ok_or_else(|| {
                            tracing::error!("failed to find remote mapping for local dependency: {}", local_dep.package);
                            tracing::error!("  expected path: {}", absolute_path_manifest.path.display());
                            tracing::error!("  available mappings:");
                            for (k, v) in &self.manifest_mappings {
                                tracing::error!("    {} -> {}:{}@{}", k.path.display(), v.registry, v.repository, v.version);
                            }
                            miette!(
                                "local dependency {} should have been made available during publish, but is not found",
                                local_dep.package
                            )
                        })?;

                    tracing::debug!(
                        "manifest mutation: {} (local:{}) -> (remote:{}:{}@{})",
                        local_dep.package,
                        local_manifest.path.display(),
                        remote_manifest.registry,
                        remote_manifest.repository,
                        remote_manifest.version
                    );

                    let remote_dependency = Dependency {
                        package: local_dep.package.clone(),
                        manifest: DependencyManifest::Remote(remote_manifest.clone()),
                    };
                    remote_dependencies.push(remote_dependency);
                    tracing::debug!("local dependency {} replaced with remote version successfully", local_dep.package);
                }
                _ => {
                    tracing::error!("unexpected dependency manifest type found for {}", local_dep.package);
                    bail!("remote dependency manifest found at an unexpected place")
                },
            }
        }

        tracing::debug!("all local dependencies replaced, total remote dependencies: {}", remote_dependencies.len());
        Ok(remote_dependencies)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::credentials::Credentials;
    use crate::manifest::{LocalDependencyManifest, RemoteDependencyManifest};
    use crate::package::PackageName;
    use semver::VersionReq;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::str::FromStr;

    fn create_test_publisher() -> Publisher {
        let registry = RegistryUri::from_str("https://test.registry.com").unwrap();
        let credentials = Credentials {
            registry_tokens: HashMap::new(),
        };
        let artifactory = Artifactory::new(registry.clone(), &credentials).unwrap();

        Publisher {
            registry,
            repository: "test-repo".to_string(),
            artifactory,
            preserve_mtime: false,
            manifest_mappings: HashMap::new(),
        }
    }

    #[test]
    fn test_replace_local_with_remote_single_dependency() {
        let mut publisher = create_test_publisher();
        let base_path = PathBuf::from("/project");

        // Setup: Add a mapping for a local dependency
        let local_manifest = LocalDependencyManifest {
            path: base_path.join("../local-lib").join(MANIFEST_FILE),
        };
        let remote_manifest = RemoteDependencyManifest {
            registry: RegistryUri::from_str("https://test.registry.com").unwrap(),
            repository: "test-repo".to_string(),
            version: VersionReq::parse("1.0.0").unwrap(),
        };
        publisher
            .manifest_mappings
            .insert(local_manifest.clone(), remote_manifest.clone());

        // Create a manifest with a local dependency
        let manifest = PackagesManifest::builder()
            .dependencies(vec![Dependency {
                package: PackageName::unchecked("local-lib"),
                manifest: DependencyManifest::Local(LocalDependencyManifest {
                    path: PathBuf::from("../local-lib"),
                }),
            }])
            .build();

        // Test: Replace local with remote
        let result = publisher
            .replace_local_with_remote_dependencies(&manifest, &base_path)
            .unwrap();

        // Verify: Should have one remote dependency
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].package, PackageName::unchecked("local-lib"));
        match &result[0].manifest {
            DependencyManifest::Remote(remote) => {
                assert_eq!(remote.repository, "test-repo");
                assert_eq!(remote.version.to_string(), "^1.0.0");
            }
            _ => panic!("Expected remote dependency"),
        }
    }

    #[test]
    fn test_replace_local_with_remote_multiple_dependencies() {
        let mut publisher = create_test_publisher();
        let base_path = PathBuf::from("/project");

        // Setup: Add mappings for two local dependencies
        let local1 = LocalDependencyManifest {
            path: base_path.join("../lib1").join(MANIFEST_FILE),
        };
        let remote1 = RemoteDependencyManifest {
            registry: RegistryUri::from_str("https://test.registry.com").unwrap(),
            repository: "test-repo".to_string(),
            version: VersionReq::parse("1.0.0").unwrap(),
        };

        let local2 = LocalDependencyManifest {
            path: base_path.join("../lib2").join(MANIFEST_FILE),
        };
        let remote2 = RemoteDependencyManifest {
            registry: RegistryUri::from_str("https://test.registry.com").unwrap(),
            repository: "test-repo".to_string(),
            version: VersionReq::parse("2.0.0").unwrap(),
        };

        publisher.manifest_mappings.insert(local1, remote1);
        publisher.manifest_mappings.insert(local2, remote2);

        // Create manifest with two local dependencies
        let manifest = PackagesManifest::builder()
            .dependencies(vec![
                Dependency {
                    package: PackageName::unchecked("lib1"),
                    manifest: DependencyManifest::Local(LocalDependencyManifest {
                        path: PathBuf::from("../lib1"),
                    }),
                },
                Dependency {
                    package: PackageName::unchecked("lib2"),
                    manifest: DependencyManifest::Local(LocalDependencyManifest {
                        path: PathBuf::from("../lib2"),
                    }),
                },
            ])
            .build();

        // Test
        let result = publisher
            .replace_local_with_remote_dependencies(&manifest, &base_path)
            .unwrap();

        // Verify: Both dependencies replaced
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].package, PackageName::unchecked("lib1"));
        assert_eq!(result[1].package, PackageName::unchecked("lib2"));
    }

    #[test]
    fn test_replace_local_with_remote_missing_mapping_fails() {
        let publisher = create_test_publisher();
        let base_path = PathBuf::from("/project");

        // Create manifest with local dependency but NO mapping
        let manifest = PackagesManifest::builder()
            .dependencies(vec![Dependency {
                package: PackageName::unchecked("missing-lib"),
                manifest: DependencyManifest::Local(LocalDependencyManifest {
                    path: PathBuf::from("../missing-lib"),
                }),
            }])
            .build();

        // Test: Should fail
        let result = publisher.replace_local_with_remote_dependencies(&manifest, &base_path);

        assert!(result.is_err());
        let err_msg = format!("{:?}", result.unwrap_err());
        assert!(err_msg.contains("missing-lib"));
        assert!(err_msg.contains("should have been made available"));
    }

    #[test]
    fn test_replace_preserves_remote_dependencies() {
        let mut publisher = create_test_publisher();
        let base_path = PathBuf::from("/project");

        // Setup: Add mapping for local dep
        let local_manifest = LocalDependencyManifest {
            path: base_path.join("../local-lib").join(MANIFEST_FILE),
        };
        let remote_manifest = RemoteDependencyManifest {
            registry: RegistryUri::from_str("https://test.registry.com").unwrap(),
            repository: "test-repo".to_string(),
            version: VersionReq::parse("1.0.0").unwrap(),
        };
        publisher
            .manifest_mappings
            .insert(local_manifest, remote_manifest.clone());

        // Create manifest with both local AND remote dependencies
        let existing_remote = Dependency {
            package: PackageName::unchecked("existing-remote"),
            manifest: DependencyManifest::Remote(RemoteDependencyManifest {
                registry: RegistryUri::from_str("https://other.registry.com").unwrap(),
                repository: "other-repo".to_string(),
                version: VersionReq::parse("3.0.0").unwrap(),
            }),
        };

        let local_dep = Dependency {
            package: PackageName::unchecked("local-lib"),
            manifest: DependencyManifest::Local(LocalDependencyManifest {
                path: PathBuf::from("../local-lib"),
            }),
        };

        let manifest = PackagesManifest::builder()
            .dependencies(vec![existing_remote.clone(), local_dep])
            .build();

        // Test
        let result = publisher
            .replace_local_with_remote_dependencies(&manifest, &base_path)
            .unwrap();

        // Verify: Should have both remote deps (existing + converted)
        assert_eq!(result.len(), 2);

        // First one should be the existing remote (unchanged)
        assert_eq!(result[0].package, PackageName::unchecked("existing-remote"));
        match &result[0].manifest {
            DependencyManifest::Remote(remote) => {
                assert_eq!(remote.repository, "other-repo");
                assert_eq!(remote.version.to_string(), "^3.0.0");
            }
            _ => panic!("Expected remote dependency"),
        }

        // Second one should be the converted local
        assert_eq!(result[1].package, PackageName::unchecked("local-lib"));
    }

    #[test]
    fn test_empty_dependencies_returns_empty() {
        let publisher = create_test_publisher();
        let base_path = PathBuf::from("/project");

        let manifest = PackagesManifest::builder()
            .dependencies(Default::default())
            .build();

        let result = publisher
            .replace_local_with_remote_dependencies(&manifest, &base_path)
            .unwrap();

        assert_eq!(result.len(), 0);
    }
}
