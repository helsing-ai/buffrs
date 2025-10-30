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

use std::{
    env,
    path::{Path, PathBuf},
    str::FromStr,
};

use miette::{Context as _, IntoDiagnostic, bail, ensure, miette};
use semver::{Version, VersionReq};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::{fs, io};

use crate::{
    credentials::Credentials,
    lock::Lockfile,
    manifest::{
        BuffrsManifest, Dependency, GenericManifest, MANIFEST_FILE, PackageManifest,
        PackagesManifest,
    },
    operations::installer::Installer,
    operations::publisher::Publisher,
    package::{PackageName, PackageStore, PackageType},
    registry::{Artifactory, RegistryUri},
};

const INITIAL_VERSION: Version = Version::new(0, 1, 0);
const BUFFRS_TESTSUITE_VAR: &str = "BUFFRS_TESTSUITE";

/// Initializes the project
pub async fn init(kind: Option<PackageType>, name: Option<PackageName>) -> miette::Result<()> {
    if PackagesManifest::exists().await? {
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

    let mut builder = PackagesManifest::builder();
    if let Some(pkg) = package {
        builder = builder.package(pkg);
    }
    let manifest = builder.dependencies(vec![]).build();

    manifest.write().await?;

    PackageStore::open(std::env::current_dir().unwrap_or_else(|_| ".".into()))
        .await
        .wrap_err("failed to create buffrs `proto` directories")?;

    Ok(())
}

/// Initializes a project with the given name in the current directory
pub async fn new(kind: Option<PackageType>, name: PackageName) -> miette::Result<()> {
    let package_dir = PathBuf::from(name.to_string());
    // create_dir fails if the folder already exists
    fs::create_dir(&package_dir)
        .await
        .into_diagnostic()
        .wrap_err_with(|| format!("failed to create {} directory", package_dir.display()))?;

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

    let mut builder = PackagesManifest::builder();
    if let Some(pkg) = package {
        builder = builder.package(pkg);
    }
    let manifest = builder.dependencies(vec![]).build();
    manifest.write_at(package_dir.as_path()).await?;

    PackageStore::open(&package_dir)
        .await
        .wrap_err("failed to create buffrs `proto` directories")?;

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
            .wrap_err_with(|| format!("invalid package name: {package}"))?;

        let version = match version {
            Some("latest") | None => DependencyLocatorVersion::Latest,
            Some(version_str) => {
                let parsed_version = VersionReq::parse(version_str)
                    .into_diagnostic()
                    .wrap_err_with(|| format!("not a valid version requirement: {version_str}"))?;
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
    let manifest_path = PathBuf::from(MANIFEST_FILE);
    let mut manifest = BuffrsManifest::require_package_manifest(&manifest_path).await?;

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
        .get_or_insert_default()
        .push(Dependency::new(registry, repository, package, version));

    manifest
        .write()
        .await
        .wrap_err_with(|| format!("failed to write `{MANIFEST_FILE}`"))?;

    Ok(())
}

/// Removes a dependency from this project
pub async fn remove(package: PackageName) -> miette::Result<()> {
    let manifest_path = PathBuf::from(MANIFEST_FILE);
    let mut manifest = BuffrsManifest::require_package_manifest(&manifest_path).await?;
    let store = PackageStore::current().await?;

    let dependency = manifest
        .dependencies
        .iter()
        .flatten()
        .position(|d| d.package == package)
        .ok_or_else(|| miette!("package {package} not in manifest"))?;

    let dependency = manifest
        .dependencies
        .get_or_insert_default()
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
    let manifest_path = PathBuf::from(MANIFEST_FILE);
    let mut manifest = BuffrsManifest::require_package_manifest(&manifest_path).await?;
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
        .wrap_err("failed to write package release to the current directory")
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
    #[cfg(feature = "git")]
    Publisher::check_git_status(allow_dirty).await?;

    let manifest = BuffrsManifest::try_read().await?;
    let current_path = env::current_dir()
        .into_diagnostic()
        .wrap_err("current dir could not be retrieved")?;

    let mut publisher = Publisher::new(registry, repository, preserve_mtime).await?;
    publisher
        .publish(&manifest, &current_path, version, dry_run)
        .await
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
    let manifest = BuffrsManifest::try_read().await?;
    let installer = Installer::new(preserve_mtime).await?;
    installer.install(&manifest).await
}

/// Uninstalls dependencies
///
/// Behavior depends on the manifest type:
/// - **Package**: Clears the package's vendor directory
/// - **Workspace**: Clears vendor directories for all workspace members
pub async fn uninstall() -> miette::Result<()> {
    let manifest = BuffrsManifest::try_read().await?;

    match manifest {
        BuffrsManifest::Package(_) => PackageStore::current().await?.clear().await,
        BuffrsManifest::Workspace(workspace_manifest) => {
            let root_path = env::current_dir()
                .into_diagnostic()
                .wrap_err("current dir could not be retrieved")?;
            let packages = workspace_manifest.workspace.resolve_members(root_path)?;

            tracing::info!(
                ":: workspace found. uninstalling dependencies for {} packages in workspace",
                packages.len()
            );

            for package_path in packages {
                tracing::info!(
                    ":: uninstalling dependencies for package: {}",
                    package_path.display()
                );

                let store = PackageStore::open(&package_path).await?;
                store.clear().await?;
            }

            Ok(())
        }
    }
}

/// Lists all protobuf files managed by Buffrs to stdout
pub async fn list() -> miette::Result<()> {
    let manifest_path = PathBuf::from(MANIFEST_FILE);
    let manifest = BuffrsManifest::require_package_manifest(&manifest_path).await?;
    let store = PackageStore::current().await?;

    if let Some(ref pkg) = manifest.package {
        store.populate(pkg).await?;
    }

    let protos = store.collect(&store.proto_vendor_path(), true).await;

    let cwd = {
        let cwd = std::env::current_dir()
            .into_diagnostic()
            .wrap_err("failed to get current directory")?;

        fs::canonicalize(cwd)
            .await
            .into_diagnostic()
            .wrap_err("failed to canonicalize current directory")?
    };

    for proto in protos.iter() {
        let rel = proto
            .strip_prefix(&cwd)
            .into_diagnostic()
            .wrap_err("failed to transform protobuf path")?;

        print!("{} ", rel.display())
    }

    Ok(())
}

/// Parses current package and validates rules.
#[cfg(feature = "validation")]
pub async fn lint() -> miette::Result<()> {
    let manifest_path = PathBuf::from(MANIFEST_FILE);
    let manifest = BuffrsManifest::require_package_manifest(&manifest_path).await?;
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
            .wrap_err("failed to read the token from the user")?;

        raw.trim().into()
    };

    credentials.registry_tokens.insert(registry.clone(), token);

    if env::var(BUFFRS_TESTSUITE_VAR).is_err() {
        Artifactory::new(registry, &credentials)?
            .ping()
            .await
            .wrap_err("failed to validate token")?;
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
