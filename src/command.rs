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
    buf_yaml::BufYamlFile,
    cache::Cache,
    config::Config,
    credentials::Credentials,
    lock::{LockedPackage, Lockfile},
    manifest::{Dependency, DependencyManifest, Manifest, PackageManifest, MANIFEST_FILE},
    package::{PackageName, PackageStore, PackageType},
    registry::{Artifactory, RegistryUri},
    resolver::{DependencyGraph, ResolvedDependency},
};

use async_recursion::async_recursion;
use miette::{bail, ensure, miette, Context, IntoDiagnostic};
use semver::{Version, VersionReq};
use std::{
    env,
    path::{Path, PathBuf},
    str::FromStr,
};
use tokio::{
    fs,
    io::{self, AsyncBufReadExt, BufReader},
};
use walkdir::WalkDir;

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

    let manifest = Manifest::new(package, vec![]);

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

    let manifest = Manifest::new(package, vec![]);
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
            .ok_or_else(|| miette!("locator \"{dependency}\" is missing a repository delimiter (use <repo>/<package>@<version>)"))?;

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
pub async fn add(
    registry: &RegistryUri,
    resolved_registry: &RegistryUri,
    dependency: &str,
) -> miette::Result<()> {
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
            let artifactory = Artifactory::new(resolved_registry, &credentials)?;

            let latest_version = artifactory
                .get_latest_version(repository.clone(), package.clone())
                .await?;
            // Convert semver::Version to semver::VersionReq. It will default to operator `>`, which is what we want for Proto.toml
            VersionReq::parse(&latest_version.to_string())
                .into_diagnostic()
                .map_err(miette::Report::from)?
        }
    };

    manifest
        .dependencies
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
        .position(|d| d.package == package)
        .ok_or_else(|| miette!("package {package} not in manifest"))?;

    let dependency = manifest.dependencies.remove(dependency);

    store.uninstall(&dependency.package).await.ok();

    manifest.write().await
}

/// Packages the api and writes it to the filesystem
pub async fn package(
    directory: impl AsRef<Path>,
    dry_run: bool,
    version: Option<Version>,
) -> miette::Result<()> {
    let mut manifest = Manifest::read().await?;
    let store = PackageStore::current().await?;

    if let Some(version) = version {
        if let Some(ref mut package) = manifest.package {
            tracing::info!(":: modified version in published manifest to {version}");

            package.version = version;
        }
    }

    if let Some(ref pkg) = manifest.package {
        store.populate(pkg).await?;
    }

    let package = store.release(&manifest).await?;

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
    registry: &RegistryUri,
    repository: String,
    #[cfg(feature = "git")] allow_dirty: bool,
    dry_run: bool,
    version: Option<Version>,
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

        let output = match output {
            Ok(output) => output,
            Err(_) => {
                return Ok(Vec::new());
            }
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
    if let Ok(statuses) = git_statuses().await {
        if !allow_dirty && !statuses.is_empty() {
            tracing::error!("{} files in the working directory contain changes that were not yet committed into git:\n", statuses.len());

            statuses.iter().for_each(|s| tracing::error!("{}", s));

            tracing::error!("\nTo proceed with publishing despite the uncommitted changes, pass the `--allow-dirty` flag\n");

            bail!("attempted to publish a dirty repository");
        }
    }

    let mut manifest = Manifest::read().await?;
    let credentials = Credentials::load().await?;
    let store = PackageStore::current().await?;
    let artifactory = Artifactory::new(registry, &credentials)?;

    if let Some(version) = version {
        if let Some(ref mut package) = manifest.package {
            tracing::info!(":: modified version in published manifest to {version}");

            package.version = version;
        }
    }

    if let Some(ref pkg) = manifest.package {
        store.populate(pkg).await?;
    }

    let package = store.release(&manifest).await?;

    if dry_run {
        tracing::warn!(":: aborting upload due to dry run");
        return Ok(());
    }

    artifactory.publish(package, repository).await
}

/// Install mode for dependencies
pub enum InstallMode {
    /// Only install dependencies, not the package itself
    DependenciesOnly,

    /// Install the package and its dependencies
    All,
}

bitflags::bitflags! {
    /// Flags for generation
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub struct GenerationFlags: u8 {
        /// Flag for generating buf.yaml files
        const BUF_YAML = 0b0001;
    }
}

/// Installs dependencies
///
/// # Arguments
/// * `mode` - The install mode (dependencies only or all)
/// * `generation` - Flags for generation of files
/// * `config` - The configuration
pub async fn install(
    mode: InstallMode,
    generation: GenerationFlags,
    config: &Config,
) -> miette::Result<()> {
    let manifest = {
        let mut manifest = Manifest::read().await?;

        // resolve all alias URLs in the manifest
        for dependency in manifest.dependencies.iter_mut() {
            if let DependencyManifest::Remote(ref mut manifest) = &mut dependency.manifest {
                manifest.registry = config.resolve_registry_uri(&manifest.registry)?;
            }
        }

        manifest
    };
    let lockfile = Lockfile::read_or_default().await?;
    let store = PackageStore::current().await?;
    let credentials = Credentials::load().await?;
    let cache = Cache::open().await?;

    store.clear().await?;

    if let InstallMode::All = mode {
        if let Some(ref pkg) = manifest.package {
            store.populate(pkg).await?;

            tracing::info!(":: installed {}@{}", pkg.name, pkg.version);
        }
    }

    let dependency_graph =
        DependencyGraph::from_manifest(&manifest, &lockfile, &credentials.into(), &cache)
            .await
            .wrap_err(miette!("dependency resolution failed"))?;

    let mut locked = Vec::new();

    #[async_recursion]
    async fn traverse_and_install(
        name: &PackageName,
        graph: &DependencyGraph,
        store: &PackageStore,
        locked: &mut Vec<LockedPackage>,
        prefix: String,
    ) -> miette::Result<()> {
        let resolved = graph.get(name).ok_or(miette!(
            "unexpected error: missing dependency in dependency graph"
        ))?;

        store.unpack(resolved.package()).await.wrap_err(miette!(
            "failed to unpack package {}",
            &resolved.package().name()
        ))?;

        tracing::info!(
            "{} installed {}@{}",
            if prefix.is_empty() { "::" } else { &prefix },
            name,
            resolved.package().version()
        );

        if let ResolvedDependency::Remote {
            package,
            registry,
            repository,
            dependants,
            ..
        } = &resolved
        {
            locked.push(package.lock(registry.clone(), repository.clone(), dependants.len()));
        }

        for (index, dependency) in resolved.depends_on().iter().enumerate() {
            let tree_char = if index + 1 == resolved.depends_on().len() {
                '┗'
            } else {
                '┣'
            };

            let new_prefix = format!(
                "{} {tree_char}",
                if prefix.is_empty() { "  " } else { &prefix }
            );

            traverse_and_install(dependency, graph, store, locked, new_prefix).await?;
        }

        Ok(())
    }

    for dependency in manifest.dependencies {
        traverse_and_install(
            &dependency.package,
            &dependency_graph,
            &store,
            &mut locked,
            String::new(),
        )
        .await?;
    }

    if let GenerationFlags::BUF_YAML = generation {
        // Check if a buf.yaml file already exists in cwd
        let mut buf_yaml = if Path::new("buf.yaml").exists() {
            BufYamlFile::from_file().wrap_err(miette!("failed to read buf.yaml file"))?
        } else {
            BufYamlFile::default()
        };

        // Add vendor modules to the Buf YAML file
        let vendor_modules = dependency_graph
            .get_package_names()
            .iter()
            .map(|p| p.to_string())
            .collect();

        buf_yaml.clear_modules();

        if manifest.package.is_some() {
            // double-check that the package really contains proto files
            // under proto/** (but not under proto/vendor/**)
            let vendor_path = store.proto_vendor_path();
            let mut has_protos = false;
            for entry in WalkDir::new(store.proto_path()) {
                if let Ok(entry) = entry {
                    if entry.path().is_file() {
                        let path = entry.path();
                        if path.starts_with(&vendor_path) {
                            continue;
                        }

                        if path.extension().map_or(false, |ext| ext == "proto") {
                            has_protos = true;
                            break;
                        }
                    }
                }
            }

            if has_protos {
                buf_yaml.add_module();
            }
        }

        buf_yaml.set_vendor_modules(vendor_modules);

        // Write the Buf YAML file
        buf_yaml
            .to_file()
            .wrap_err(miette!("failed to write buf.yaml file"))?;
    }

    Lockfile::from_iter(locked.into_iter()).write().await
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

    // Canonicalize the protos
    let protos = protos
        .into_iter()
        .map(|proto| {
            proto
                .canonicalize()
                .into_diagnostic()
                .wrap_err(miette!("failed to canonicalize proto path"))
        })
        .collect::<miette::Result<Vec<_>>>()?;

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
///
/// # Arguments
///  * `registry` - The registry to log in to
///  * `token` - An optional token to use, if not provided, the user will be prompted for one
pub async fn login(registry: &RegistryUri, token: Option<String>) -> miette::Result<()> {
    let mut credentials = Credentials::load().await?;

    let token = match token {
        Some(token) => token,
        None => {
            tracing::info!(":: please enter your artifactory token:");

            let mut raw = String::new();
            let mut reader = BufReader::new(io::stdin());

            reader
                .read_line(&mut raw)
                .await
                .into_diagnostic()
                .wrap_err(miette!("failed to read the token from the user"))?;

            raw.trim().into()
        }
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
pub async fn logout(registry: &RegistryUri) -> miette::Result<()> {
    let mut credentials = Credentials::load().await?;
    credentials.registry_tokens.remove(registry);
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
        assert!("repo-with-dash/pkg@=1.0"
            .parse::<DependencyLocator>()
            .is_ok());
        assert!("repo-with-dash/pkg-with-dash@=1.0"
            .parse::<DependencyLocator>()
            .is_ok());
        assert!("repo/pkg@=1.0.0-with-prerelease"
            .parse::<DependencyLocator>()
            .is_ok());
        assert!("repo/pkg@latest".parse::<DependencyLocator>().is_ok());
        assert!("repo/pkg".parse::<DependencyLocator>().is_ok());
    }

    #[test]
    fn invalid_dependency_locators() {
        assert!("/xyz@1.0.0".parse::<DependencyLocator>().is_err());
        assert!("repo/@1.0.0".parse::<DependencyLocator>().is_err());
        assert!("repo@1.0.0".parse::<DependencyLocator>().is_err());
        assert!("repo/pkg@latestwithtypo"
            .parse::<DependencyLocator>()
            .is_err());
        assert!("repo/pkg@=1#meta".parse::<DependencyLocator>().is_err());
        assert!("repo/PKG@=1.0".parse::<DependencyLocator>().is_err());
    }
}
