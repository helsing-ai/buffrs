// (c) Copyright 2025 Helsing GmbH. All rights reserved.

use std::collections::HashMap;
use std::env;
use std::path::Path;
#[cfg(feature = "git")]
use std::process::Stdio;
use std::str::FromStr;

use miette::{Context as _, IntoDiagnostic, bail, miette};
use semver::{Version, VersionReq};

use crate::credentials::Credentials;
use crate::manifest::{
    Dependency, DependencyManifest, LocalDependencyManifest, MANIFEST_FILE, Manifest, ManifestType,
    RemoteDependencyManifest,
};
use crate::package::PackageStore;
use crate::registry::{Artifactory, RegistryUri};
use crate::resolver::{DependencyGraph, DependencySource};

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
        let credentials = Credentials::load().await?;
        let artifactory = Artifactory::new(registry.clone(), &credentials)?;

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
        let statuses = Self::get_uncommitted_files().await?;

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
        manifest: &Manifest,
        package_path: &Path,
        version: Option<Version>,
        dry_run: bool,
    ) -> miette::Result<()> {
        if dry_run {
            tracing::warn!(":: aborting upload due to dry run");
            return Ok(());
        }

        match manifest.manifest_type {
            ManifestType::Package => {
                self.publish_package_from_manifest(manifest, package_path, version)
                    .await
            }
            ManifestType::Workspace => {
                if version.is_some() {
                    tracing::warn!(":: version flag is ignored for workspace publishes");
                }
                self.publish_workspace_from_manifest(manifest).await
            }
        }
    }

    /// Publishes a single package from its manifest
    async fn publish_package_from_manifest(
        &mut self,
        manifest: &Manifest,
        package_path: &Path,
        version: Option<Version>,
    ) -> miette::Result<()> {
        let mut root_manifest = manifest.clone();
        let store = PackageStore::current().await?;

        if let Some(version) = version
            && let Some(ref mut package) = root_manifest.package
        {
            tracing::info!(":: modified version in published manifest to {version}");
            package.version = version;
        }

        // Build dependency graph
        let credentials = Credentials::load().await?;
        let graph = DependencyGraph::build(&root_manifest, package_path, &credentials).await?;
        let ordered_dependencies = graph.ordered_dependencies()?;

        // Publish local dependencies first
        for dependency in ordered_dependencies {
            if let DependencySource::Local {
                path: absolute_path,
            } = dependency.node.source
            {
                self.publish_package_at_path(&absolute_path).await?;
            }
        }

        // Populate and publish the root package
        if let Some(ref pkg) = root_manifest.package {
            store.populate(pkg).await?;
        }

        self.publish_package_at_path(package_path).await?;

        Ok(())
    }

    /// Publishes all packages in a workspace
    async fn publish_workspace_from_manifest(&mut self, manifest: &Manifest) -> miette::Result<()> {
        let workspace = manifest.workspace.as_ref().ok_or_else(|| {
            miette!("publish_workspace called on manifest that does not define a workspace")
        })?;

        let root_path = env::current_dir()
            .into_diagnostic()
            .wrap_err("current dir could not be retrieved")?;
        let packages = workspace.resolve_members(root_path)?;

        tracing::info!(
            ":: workspace found. publishing {} packages in workspace",
            packages.len()
        );

        let credentials = Credentials::load().await?;

        // Iterate through each workspace member
        for member_path in packages {
            tracing::info!(":: processing workspace member: {}", member_path.display());

            let member_manifest = Manifest::try_read_from(member_path.join(MANIFEST_FILE)).await?;

            // Build dependency graph for this member
            let graph =
                DependencyGraph::build(&member_manifest, &member_path, &credentials).await?;
            let dependencies = graph.ordered_dependencies()?;

            // Publish local dependencies first
            for dependency in dependencies {
                if let DependencySource::Local {
                    path: absolute_path,
                } = dependency.node.source
                {
                    self.publish_package_at_path(&absolute_path).await?;
                }
            }

            // Populate and publish the member itself
            if let Some(ref pkg) = member_manifest.package {
                let member_store = PackageStore::open(&member_path).await?;
                member_store.populate(pkg).await?;
            }

            self.publish_package_at_path(&member_path).await?;
        }

        Ok(())
    }

    /// Publishes a local package at the given path
    ///
    /// This method:
    /// 1. Checks if already published (idempotent)
    /// 2. Reads the package manifest
    /// 3. Replaces local dependencies with their published remote versions
    /// 4. Creates a package with the updated manifest
    /// 5. Publishes to the registry
    /// 6. Records the mapping of local path to remote location
    async fn publish_package_at_path(&mut self, package_path: &Path) -> miette::Result<()> {
        let manifest_path = package_path.join(MANIFEST_FILE);

        // Check if this package has already been published (idempotent)
        let local_manifest = LocalDependencyManifest {
            path: manifest_path.clone(),
        };
        if self.manifest_mappings.contains_key(&local_manifest) {
            tracing::debug!(
                ":: package at {} already published, skipping",
                package_path.display()
            );
            return Ok(());
        }

        let manifest = Manifest::try_read_from(&manifest_path)
            .await
            .wrap_err_with(|| {
                format!("failed to read manifest file at {}", package_path.display())
            })?;

        // Create a store at the package's path
        let package_store = PackageStore::open(package_path).await?;

        let remote_dependencies =
            self.replace_local_with_remote_dependencies(&manifest, package_path)?;

        // Clone manifest with local dependencies replaced by their remote locations
        let remote_deps_manifest = manifest.clone_with_different_dependencies(remote_dependencies);
        let package = package_store
            .release(&remote_deps_manifest, self.preserve_mtime)
            .await?;

        self.artifactory
            .publish(package.clone(), self.repository.clone())
            .await
            .wrap_err_with(|| format!("publishing of package {} failed", package.name()))?;

        // Store the mapping for this package
        let package_version =
            VersionReq::from_str(&package.version().to_string()).into_diagnostic()?;

        let remote_manifest = RemoteDependencyManifest {
            version: package_version,
            registry: self.registry.clone(),
            repository: self.repository.clone(),
        };

        self.manifest_mappings
            .insert(local_manifest, remote_manifest);

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

                    let remote_manifest = self
                        .manifest_mappings
                        .get(&absolute_path_manifest)
                        .ok_or_else(|| {
                            miette!(
                                "local dependency {} should have been made available during publish, but is not found",
                                local_dep.package
                            )
                        })?;

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
        let manifest = Manifest::builder()
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
        let manifest = Manifest::builder()
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
        let manifest = Manifest::builder()
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

        let manifest = Manifest::builder()
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

        let manifest = Manifest::builder().dependencies(vec![]).build();

        let result = publisher
            .replace_local_with_remote_dependencies(&manifest, &base_path)
            .unwrap();

        assert_eq!(result.len(), 0);
    }
}
