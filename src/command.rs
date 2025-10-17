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

use crate::{
    cache::{Cache, Entry as CacheEntry},
    credentials::Credentials,
    lock::Lockfile,
    manifest::{Dependency, MANIFEST_FILE, Manifest, PackageManifest},
    package::{Package, PackageName, PackageStore, PackageType},
    registry::{Artifactory, RegistryUri},
    resolver,
};

use crate::lock::LOCKFILE;
use crate::manifest::{
    DependencyManifest, LocalDependencyManifest, ManifestType, RemoteDependencyManifest,
};
use crate::resolver::{DependencyDetails, DependencySource};
use miette::{Context as _, IntoDiagnostic, Report, bail, ensure, miette};
use semver::{Version, VersionReq};
use std::collections::HashMap;
use std::{
    env,
    path::{Path, PathBuf},
    str::FromStr,
};
use tokio::{
    fs,
    io::{self, AsyncBufReadExt, BufReader},
};

const INITIAL_VERSION: Version = Version::new(0, 1, 0);
const BUFFRS_TESTSUITE_VAR: &str = "BUFFRS_TESTSUITE";

/// Initializes the project
pub async fn init(kind: Option<PackageType>, name: Option<PackageName>) -> miette::Result<()> {
    if Manifest::exists().await? {
        bail!("a manifest file was found, project is already initialized");
    }

    fn curr_dir_name() -> miette::Result<PackageName> {
        std::env::current_dir()
            .into_diagnostic()?
            .file_name()
            // because the path originates from the current directory, this condition is never met
            .ok_or(miette!(
                "unexpected error: current directory path terminates in .."
            ))?
            .to_str()
            .ok_or_else(|| miette!("current directory path is not valid utf-8"))?
            .parse()
    }

    let package = kind
        .map(|kind| -> miette::Result<PackageManifest> {
            let name = name.map(Result::Ok).unwrap_or_else(curr_dir_name)?;

            Ok(PackageManifest {
                kind,
                name,
                version: INITIAL_VERSION,
                description: None,
            })
        })
        .transpose()?;

    let mut builder = Manifest::builder();
    if let Some(pkg) = package {
        builder = builder.package(pkg);
    }
    let manifest = builder.dependencies(vec![]).build();

    manifest.write().await?;

    PackageStore::open(std::env::current_dir().unwrap_or_else(|_| ".".into()))
        .await
        .wrap_err(miette!("failed to create buffrs `proto` directories"))?;

    Ok(())
}

/// Initializes a project with the given name in the current directory
pub async fn new(kind: Option<PackageType>, name: PackageName) -> miette::Result<()> {
    let package_dir = PathBuf::from(name.to_string());
    // create_dir fails if the folder already exists
    fs::create_dir(&package_dir)
        .await
        .into_diagnostic()
        .wrap_err(miette!(
            "failed to create {} directory",
            package_dir.display()
        ))?;

    let package = kind
        .map(|kind| -> miette::Result<PackageManifest> {
            Ok(PackageManifest {
                kind,
                name,
                version: INITIAL_VERSION,
                description: None,
            })
        })
        .transpose()?;

    let mut builder = Manifest::builder();
    if let Some(pkg) = package {
        builder = builder.package(pkg);
    }
    let manifest = builder.dependencies(vec![]).build();
    manifest.write_at(&package_dir).await?;

    PackageStore::open(&package_dir)
        .await
        .wrap_err(miette!("failed to create buffrs `proto` directories"))?;

    Ok(())
}

struct DependencyLocator {
    repository: String,
    package: PackageName,
    version: DependencyLocatorVersion,
}

enum DependencyLocatorVersion {
    Version(VersionReq),
    Latest,
}

impl FromStr for DependencyLocator {
    type Err = miette::Report;

    fn from_str(dependency: &str) -> miette::Result<Self> {
        let lower_kebab = |c: char| (c.is_lowercase() && c.is_ascii_alphabetic()) || c == '-';

        let (repository, dependency) = dependency
            .trim()
            .split_once('/')
            .ok_or_else(|| miette!("locator {dependency} is missing a repository delimiter"))?;

        ensure!(
            repository.chars().all(lower_kebab),
            "repository {repository} is not in kebab case"
        );

        ensure!(!repository.is_empty(), "repository must not be empty");

        let repository = repository.into();

        let (package, version) = dependency
            .split_once('@')
            .map(|(package, version)| (package, Some(version)))
            .unwrap_or_else(|| (dependency, None));

        let package = package
            .parse::<PackageName>()
            .wrap_err(miette!("invalid package name: {package}"))?;

        let version = match version {
            Some("latest") | None => DependencyLocatorVersion::Latest,
            Some(version_str) => {
                let parsed_version = VersionReq::parse(version_str)
                    .into_diagnostic()
                    .wrap_err(miette!("not a valid version requirement: {version_str}"))?;
                DependencyLocatorVersion::Version(parsed_version)
            }
        };

        Ok(Self {
            repository,
            package,
            version,
        })
    }
}

/// Adds a dependency to this project
pub async fn add(registry: RegistryUri, dependency: &str) -> miette::Result<()> {
    let mut manifest = Manifest::read().await?;

    let DependencyLocator {
        repository,
        package,
        version,
    } = dependency.parse()?;

    let version = match version {
        DependencyLocatorVersion::Version(version_req) => version_req,
        DependencyLocatorVersion::Latest => {
            // query artifactory to retrieve the actual latest version
            let credentials = Credentials::load().await?;
            let artifactory = Artifactory::new(registry.clone(), &credentials)?;

            let latest_version = artifactory
                .get_latest_version(repository.clone(), package.clone())
                .await?;
            // Convert semver::Version to semver::VersionReq. It will default to operator `>`, which is what we want for Proto.toml
            VersionReq::parse(&latest_version.to_string()).into_diagnostic()?
        }
    };

    manifest
        .dependencies
        .get_or_insert_with(Vec::new)
        .push(Dependency::new(registry, repository, package, version));

    manifest
        .write()
        .await
        .wrap_err(miette!("failed to write `{MANIFEST_FILE}`"))
}

/// Removes a dependency from this project
pub async fn remove(package: PackageName) -> miette::Result<()> {
    let mut manifest = Manifest::read().await?;
    let store = PackageStore::current().await?;

    let dependency = manifest
        .dependencies
        .iter()
        .flatten()
        .position(|d| d.package == package)
        .ok_or_else(|| miette!("package {package} not in manifest"))?;

    let dependency = manifest
        .dependencies
        .get_or_insert_with(Vec::new)
        .remove(dependency);

    store.uninstall(&dependency.package).await.ok();

    manifest.write().await
}

/// Packages the api and writes it to the filesystem
pub async fn package(
    directory: impl AsRef<Path>,
    dry_run: bool,
    version: Option<Version>,
    preserve_mtime: bool,
) -> miette::Result<()> {
    let mut manifest = Manifest::read().await?;
    let store = PackageStore::current().await?;

    if let Some(version) = version
        && let Some(ref mut package) = manifest.package
    {
        tracing::info!(":: modified version in published manifest to {version}");

        package.version = version;
    }

    if let Some(ref pkg) = manifest.package {
        store.populate(pkg).await?;
    }

    let package = store.release(&manifest, preserve_mtime).await?;

    if dry_run {
        return Ok(());
    }

    let path = {
        let file = format!("{}-{}.tgz", package.name(), package.version());

        directory.as_ref().join(file)
    };

    fs::write(path, package.tgz)
        .await
        .into_diagnostic()
        .wrap_err(miette!(
            "failed to write package release to the current directory"
        ))
}

/// Publishes the api package to the registry
pub async fn publish(
    registry: RegistryUri,
    repository: String,
    #[cfg(feature = "git")] allow_dirty: bool,
    dry_run: bool,
    version: Option<Version>,
    preserve_mtime: bool,
) -> miette::Result<()> {
    let manifest = Manifest::read().await?;
    let current_path = env::current_dir()
        .into_diagnostic()
        .wrap_err("current dir could not be retrieved")?;

    match manifest.manifest_type {
        ManifestType::Package => {
            publish_package(
                registry,
                repository,
                #[cfg(feature = "git")]
                allow_dirty,
                dry_run,
                version,
                &current_path,
                preserve_mtime,
            )
            .await
        }
        ManifestType::Workspace => {
            publish_workspace(
                registry,
                repository,
                #[cfg(feature = "git")]
                allow_dirty,
                dry_run,
                version,
                &current_path,
                preserve_mtime,
            )
            .await
        }
    }
}

async fn publish_workspace(
    _registry: RegistryUri,
    _repository: String,
    #[cfg(feature = "git")] _allow_dirty: bool,
    _dry_run: bool,
    _version: Option<Version>,
    _package_path: &PathBuf,
    _preserve_mtime: bool,
) -> miette::Result<()> {
    tracing::warn!("buffrs publish not implemented yet");
    Ok(())
}

/// Publishes the api package to the registry
async fn publish_package(
    registry: RegistryUri,
    repository: String,
    #[cfg(feature = "git")] allow_dirty: bool,
    dry_run: bool,
    version: Option<Version>,
    package_path: &PathBuf,
    preserve_mtime: bool,
) -> miette::Result<()> {
    #[cfg(feature = "git")]
    async fn git_statuses() -> miette::Result<Vec<String>> {
        use std::process::Stdio;

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

    #[cfg(feature = "git")]
    if let Ok(statuses) = git_statuses().await
        && !allow_dirty
        && !statuses.is_empty()
    {
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

    let mut root_manifest = Manifest::read().await?;
    let credentials = Credentials::load().await?;
    let store = PackageStore::current().await?;
    let artifactory = Artifactory::new(registry.clone(), &credentials)?;

    if let Some(version) = version
        && let Some(ref mut package) = root_manifest.package
    {
        tracing::info!(":: modified version in published manifest to {version}");

        package.version = version;
    }

    if dry_run {
        tracing::warn!(":: aborting upload due to dry run");
        return Ok(());
    }

    // ### Logic starts here. ###

    // 1. Build graph
    let graph_v2 =
        resolver::DependencyGraph::build(&root_manifest, package_path, &credentials).await?;

    // 2. Topo sort
    let ordered_dependencies = graph_v2.ordered_dependencies()?;

    // 3. Initialize mapping M to store the remote location of published local packages
    let mut manifest_mappings: HashMap<LocalDependencyManifest, RemoteDependencyManifest> =
        HashMap::new();

    // tracing::info!("{:?}", ordered_dependencies);
    // tracing::info!("{:?}", manifest_mappings);

    // 3. Iterate through dependency D and publish local dependencies
    for dependency in ordered_dependencies {
        match dependency.node.source {
            DependencySource::Local { path: absolute_path } => {
                let abs_manifest_path = absolute_path.join(MANIFEST_FILE);
                let dep_manifest = Manifest::try_read_from(&abs_manifest_path)
                    .await
                    .wrap_err(miette!(
                        "Failed to read manifest file at {}",
                        absolute_path.display()
                    ))?;

                if let Some(ref pkg) = dep_manifest.package {
                    store.populate(pkg).await?;
                }

                let remote_dependencies =
                    replace_local_with_remote_dependencies(&mut manifest_mappings, &dep_manifest, package_path)?;

                // Cloned package where local dependencies have been updated with their remote locations in the manifest
                let remote_deps_manifest =
                    dep_manifest.clone_with_different_dependencies(remote_dependencies);

                let package = store.release(&remote_deps_manifest, preserve_mtime).await?;
                // tracing::info!("0");

                artifactory
                    .publish(package.clone(), repository.clone())
                    .await
                    .wrap_err(miette!("publishing of package {} failed", package.name()))?;


                let local_manifest = LocalDependencyManifest {
                    path: abs_manifest_path
                };
                // tracing::info!("Local: {:?}", local_manifest);

                let package_version = VersionReq::from_str(package.version().to_string().as_str())
                    .into_diagnostic()?;

                // tracing::info!("B");

                let remote_manifest = RemoteDependencyManifest {
                    version: package_version,
                    registry: registry.clone(),
                    repository: repository.clone(),
                };

                // tracing::info!("C");

                manifest_mappings.insert(local_manifest, remote_manifest);
            }
            // Only local packages get published
            DependencySource::Remote { .. } => {
                // Remote dependencies don't need to be published, skip them
                continue;
            }
        }
    }



    // 4. Publish the root package itself with updated dependencies
    if let Some(ref pkg) = root_manifest.package {
        store.populate(pkg).await?;
    }

    let root_remote_dependencies =
        replace_local_with_remote_dependencies(&mut manifest_mappings, &root_manifest, package_path)?;

    let root_manifest_with_remote_deps =
        root_manifest.clone_with_different_dependencies(root_remote_dependencies);
    let root_package = store.release(&root_manifest_with_remote_deps, preserve_mtime).await?;

    artifactory
        .publish(root_package.clone(), repository.clone())
        .await
        .wrap_err(miette!("publishing of package {} failed", root_package.name()))?;

    Ok(())
}

fn replace_local_with_remote_dependencies(
    remote_manifests: &mut HashMap<LocalDependencyManifest, RemoteDependencyManifest>,
    manifest: &Manifest,
    base_path: &Path,
) -> Result<Vec<Dependency>, Report> {
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

                // tracing::info!("Key: {:?}", absolute_path_manifest);
                // tracing::info!("HashMap: {:?}", remote_manifests);
                let remote_manifest = remote_manifests
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

/// Installs dependencies for the current project
///
/// Behavior depends on the manifest type:
/// - **Package**: Installs dependencies listed in the `[dependencies]` section
/// - **Workspace**: Installs dependencies for all workspace members
///
/// # Arguments
///
/// * `preserve_mtime` - If true, local dependencies preserve their modification time
pub async fn install(preserve_mtime: bool) -> miette::Result<()> {
    let manifest = Manifest::read().await?;

    match manifest.manifest_type {
        ManifestType::Package => {
            let lockfile = Lockfile::read_or_default().await?;
            let store = PackageStore::current().await?;
            let cache = Cache::open().await?;
            let current_path = env::current_dir()
                .into_diagnostic()
                .wrap_err("current dir could not be retrieved")?;

            install_package(
                preserve_mtime,
                &manifest,
                &lockfile,
                &store,
                &current_path,
                &cache,
            )
            .await
        }
        ManifestType::Workspace => install_workspace(preserve_mtime, &manifest).await,
    }
}

/// Installs dependencies for a workspace (not yet implemented)
async fn install_workspace(preserve_mtime: bool, manifest: &Manifest) -> miette::Result<()> {
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

    let cache = Cache::open().await?;

    for package in packages {
        let canonical_name = fs::canonicalize(&package).await.into_diagnostic()?;
        let pkg_manifest = Manifest::try_read_from(package.join(MANIFEST_FILE)).await?;
        let pkg_lockfile = Lockfile::read_from_or_default(package.join(LOCKFILE)).await?;
        let store = PackageStore::open(&package).await?;

        tracing::info!(
            ":: running install for package: {}",
            canonical_name.to_str().unwrap()
        );
        install_package(
            preserve_mtime,
            &pkg_manifest,
            &pkg_lockfile,
            &store,
            &package,
            &cache,
        )
        .await?
    }

    Ok(())
}

/// Installs dependencies of a package
///
/// if [preserve_mtime] is true, local dependencies will keep their modification time
async fn install_package(
    preserve_mtime: bool,
    manifest: &Manifest,
    lockfile: &Lockfile,
    store: &PackageStore,
    package_path: &PathBuf,
    cache: &Cache,
) -> miette::Result<()> {
    let credentials = Credentials::load().await?;

    store.clear().await?;

    if let Some(ref pkg) = manifest.package {
        store.populate(pkg).await?;

        tracing::info!(":: installed {}@{}", pkg.name, pkg.version);
    }

    let graph_v2 = resolver::DependencyGraph::build(manifest, package_path, &credentials).await?;
    let dependencies = graph_v2.ordered_dependencies()?;

    let mut locked = Vec::new();

    for dependency_node in dependencies {
        // Iterate through the dependencies in order and install them
        let package = match dependency_node.node.source {
            // corresponds to process_local_dependency in resolver.rs:
            // Key logic is: Read manifest, create directory structures via PackageStore::open, initialize package via store.release
            DependencySource::Local { path } => {
                // For local dependencies, create a store at the dependency path and release it
                let dep_manifest = Manifest::try_read_from(path.join(MANIFEST_FILE)).await?;
                let dep_store = PackageStore::open(&path).await?;
                dep_store.release(&dep_manifest, preserve_mtime).await?
            }

            // corresponds to process_remote_dependency in resolver.rs:
            DependencySource::Remote {
                repository,
                registry,
            } => {
                let package_name = &dependency_node.node.name;
                let version = &dependency_node.node.version;

                // Try to use cached package if available in lockfile
                let mut resolved_package = None;
                if let Some(locked_entry) = lockfile.get(package_name) {
                    // Validate registry consistency
                    ensure!(
                        registry == locked_entry.registry,
                        "registry mismatch for {}: manifest specifies {} but lockfile requires {}",
                        package_name,
                        registry,
                        locked_entry.registry
                    );

                    // Try to retrieve from cache if version matches lockfile
                    if version.matches(&locked_entry.version) {
                        if let Some(cached_pkg) = cache.get(locked_entry.into()).await? {
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
                let resolved_package = match resolved_package {
                    Some(pkg) => pkg,
                    None => {
                        let artifactory = Artifactory::new(registry.clone(), &credentials)
                            .wrap_err(miette!("failed to initialize registry {}", registry))?;

                        download_and_cache(
                            &artifactory,
                            package_name,
                            &registry,
                            &repository,
                            version,
                            cache,
                        )
                        .await?
                    }
                };

                // Add to new lockfile
                let dependants_count = graph_v2.dependants_count_of(package_name);
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

/// Downloads a package from the registry and caches it
async fn download_and_cache(
    artifactory: &Artifactory,
    package_name: &PackageName,
    registry: &RegistryUri,
    repository: &str,
    version: &VersionReq,
    cache: &Cache,
) -> miette::Result<Package> {
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
    cache
        .put(cache_key, downloaded_package.tgz.clone())
        .await
        .ok();

    Ok(downloaded_package)
}

/// Uninstalls dependencies
pub async fn uninstall() -> miette::Result<()> {
    PackageStore::current().await?.clear().await
}

/// Lists all protobuf files managed by Buffrs to stdout
pub async fn list() -> miette::Result<()> {
    let store = PackageStore::current().await?;
    let manifest = Manifest::read().await?;

    if let Some(ref pkg) = manifest.package {
        store.populate(pkg).await?;
    }

    let protos = store.collect(&store.proto_vendor_path(), true).await;

    let cwd = {
        let cwd = std::env::current_dir()
            .into_diagnostic()
            .wrap_err(miette!("failed to get current directory"))?;

        fs::canonicalize(cwd)
            .await
            .into_diagnostic()
            .wrap_err(miette!("failed to canonicalize current directory"))?
    };

    for proto in protos.iter() {
        let rel = proto
            .strip_prefix(&cwd)
            .into_diagnostic()
            .wrap_err(miette!("failed to transform protobuf path"))?;

        print!("{} ", rel.display())
    }

    Ok(())
}

/// Parses current package and validates rules.
#[cfg(feature = "validation")]
pub async fn lint() -> miette::Result<()> {
    let manifest = Manifest::read().await?;
    let store = PackageStore::current().await?;

    let pkg = manifest.package.ok_or(miette!(
        "a [package] section must be declared run the linter"
    ))?;

    store.populate(&pkg).await?;
    let violations = store.validate(&pkg).await?;

    violations
        .into_iter()
        .map(miette::Report::new)
        .for_each(|r| eprintln!("{r:?}"));

    Ok(())
}

/// Logs you in for a registry
pub async fn login(registry: RegistryUri) -> miette::Result<()> {
    let mut credentials = Credentials::load().await?;

    tracing::info!(":: please enter your artifactory token:");

    let token = {
        let mut raw = String::new();
        let mut reader = BufReader::new(io::stdin());

        reader
            .read_line(&mut raw)
            .await
            .into_diagnostic()
            .wrap_err(miette!("failed to read the token from the user"))?;

        raw.trim().into()
    };

    credentials.registry_tokens.insert(registry.clone(), token);

    if env::var(BUFFRS_TESTSUITE_VAR).is_err() {
        Artifactory::new(registry, &credentials)?
            .ping()
            .await
            .wrap_err(miette!("failed to validate token"))?;
    }

    credentials.write().await
}

/// Logs you out from a registry
pub async fn logout(registry: RegistryUri) -> miette::Result<()> {
    let mut credentials = Credentials::load().await?;
    credentials.registry_tokens.remove(&registry);
    credentials.write().await
}

/// Commands on the lockfile
pub mod lock {
    use super::*;
    use crate::lock::FileRequirement;

    /// Prints the file requirements serialized as JSON
    pub async fn print_files() -> miette::Result<()> {
        let lock = Lockfile::read().await?;

        let requirements: Vec<FileRequirement> = lock.into();

        // hint: always ok, as per serde_json doc
        if let Ok(json) = serde_json::to_string_pretty(&requirements) {
            println!("{json}");
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::DependencyLocator;

    #[test]
    fn valid_dependency_locator() {
        assert!("repo/pkg@1.0.0".parse::<DependencyLocator>().is_ok());
        assert!("repo/pkg@=1.0".parse::<DependencyLocator>().is_ok());
        assert!(
            "repo-with-dash/pkg@=1.0"
                .parse::<DependencyLocator>()
                .is_ok()
        );
        assert!(
            "repo-with-dash/pkg-with-dash@=1.0"
                .parse::<DependencyLocator>()
                .is_ok()
        );
        assert!(
            "repo/pkg@=1.0.0-with-prerelease"
                .parse::<DependencyLocator>()
                .is_ok()
        );
        assert!("repo/pkg@latest".parse::<DependencyLocator>().is_ok());
        assert!("repo/pkg".parse::<DependencyLocator>().is_ok());
    }

    #[test]
    fn invalid_dependency_locators() {
        assert!("/xyz@1.0.0".parse::<DependencyLocator>().is_err());
        assert!("repo/@1.0.0".parse::<DependencyLocator>().is_err());
        assert!("repo@1.0.0".parse::<DependencyLocator>().is_err());
        assert!(
            "repo/pkg@latestwithtypo"
                .parse::<DependencyLocator>()
                .is_err()
        );
        assert!("repo/pkg@=1#meta".parse::<DependencyLocator>().is_err());
        assert!("repo/PKG@=1.0".parse::<DependencyLocator>().is_err());
    }
}
