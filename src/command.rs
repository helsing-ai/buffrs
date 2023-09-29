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
    credentials::{self, Credentials},
    errors::IoError,
    lock::{self, LockedPackage, Lockfile},
    manifest::{self, Dependency, Manifest, PackageManifest},
    package::{
        self, DependencyGraph, PackageName, PackageNameValidationError, PackageStore, PackageType,
    },
    registry::{self, Artifactory, Registry, RegistryUri},
};
#[cfg(feature = "build")]
use crate::{generator, Language};
use async_recursion::async_recursion;
use semver::{Version, VersionReq};
use std::{env, path::Path, sync::Arc};
use thiserror::Error;

const INITIAL_VERSION: Version = Version::new(0, 1, 0);

#[derive(Error, Debug)]
pub enum InitError {
    #[error("A manifest file is already present in the current director and won't be overriden")]
    ManifestExists,
    #[error("{0}")]
    Io(IoError),
    #[error("Failed to write manifest. Cause: {0}")]
    ManifestWrite(manifest::WriteError),
    #[error("Path is not valid a UTF-8 string")]
    Utf8Error,
    #[error("Invalid package name. {0}")]
    PackageNameValidation(PackageNameValidationError),
}

/// Initializes the project
pub async fn init(kind: PackageType, name: Option<PackageName>) -> Result<(), InitError> {
    if !Manifest::exists().await.map_err(InitError::Io)? {
        return Err(InitError::ManifestExists);
    }

    const DIR_ERR: &str = "Failed to read current directory name";

    fn curr_dir_name() -> Result<PackageName, InitError> {
        std::env::current_dir()
            .map_err(|err| InitError::Io(IoError::new(err, "Failed to read current directory")))?
            .file_name()
            .expect("Unexpected error: current directory path terminates in ..")
            .to_str()
            .ok_or(InitError::Utf8Error)?
            .parse()
            .map_err(InitError::PackageNameValidation)
    }

    let name = name.map(Result::Ok).unwrap_or_else(curr_dir_name)?;

    let manifest = Manifest {
        package: PackageManifest {
            kind,
            name,
            version: INITIAL_VERSION,
            description: None,
        },
        dependencies: vec![],
    };

    manifest.write().await.map_err(InitError::ManifestWrite)?;

    PackageStore::create().await.map_err(InitError::Io)
}

#[derive(Error, Debug)]
pub enum AddError {
    #[error("Locator {0} is missing a repository delimiter")]
    MissingRepository(String),
    #[error("Repository {0} is not in kebab case")]
    NotKebabCase(String),
    #[error("Dependency specification is missing version part: {0}")]
    MissingVersion(String),
    #[error("Failed to validate dependency name. {0}")]
    InvalidName(package::PackageNameValidationError),
    #[error("Not a valid version requirement: {given}. Cause: {source}")]
    InvalidVersionReq {
        given: String,
        source: semver::Error,
    },
    #[error("Failed to read manifest. {0}")]
    ManifestRead(manifest::ReadError),
    #[error("Failed to write manifest. {0}")]
    ManifestWrite(manifest::WriteError),
}

/// Adds a dependency to this project
pub async fn add(registry: RegistryUri, dependency: &str) -> Result<(), AddError> {
    let lower_kebab = |c: char| (c.is_lowercase() && c.is_ascii_alphabetic()) || c == '-';

    let (repository, dependency) = dependency
        .trim()
        .split_once('/')
        .ok_or(AddError::MissingRepository(dependency.into()))?;

    if !repository.chars().all(lower_kebab) {
        return Err(AddError::NotKebabCase(repository.into()));
    }

    let repository = repository.into();

    let (package, version) = dependency
        .split_once('@')
        .ok_or(AddError::MissingVersion(dependency.into()))?;

    let package = package
        .parse::<PackageName>()
        .map_err(AddError::InvalidName)?;

    let version = version
        .parse::<VersionReq>()
        .map_err(|err| AddError::InvalidVersionReq {
            given: version.into(),
            source: err,
        })?;

    let mut manifest = Manifest::read().await.map_err(AddError::ManifestRead)?;

    manifest
        .dependencies
        .push(Dependency::new(registry, repository, package, version));

    manifest.write().await.map_err(AddError::ManifestWrite)
}

#[derive(Error, Debug)]
pub enum RemoveError {
    #[error("Failed to read manifest. {0}")]
    ManifestRead(manifest::ReadError),
    #[error("Package {0} not in manifest")]
    PackageNotFound(PackageName),
    #[error("Failed to write manifest. {0}")]
    ManifestWrite(manifest::WriteError),
}

/// Removes a dependency from this project
pub async fn remove(package: PackageName) -> Result<(), RemoveError> {
    let mut manifest = Manifest::read().await.map_err(RemoveError::ManifestRead)?;

    let match_idx = manifest
        .dependencies
        .iter()
        .position(|d| d.package == package)
        .ok_or(RemoveError::PackageNotFound(package))?;

    let dependency = manifest.dependencies.remove(match_idx);

    PackageStore::uninstall(&dependency.package).await.ok();

    manifest.write().await.map_err(RemoveError::ManifestWrite)
}

#[derive(Error, Debug)]
pub enum PackageError {
    #[error("Failed to release package. {0}")]
    Release(package::ReleaseError),
    #[error("Failed to write package to filesystem. {0}")]
    Io(std::io::Error),
}

/// Packages the api and writes it to the filesystem
pub async fn package(directory: impl AsRef<Path>, dry_run: bool) -> Result<(), PackageError> {
    let package = PackageStore::release()
        .await
        .map_err(PackageError::Release)?;

    let path = directory.as_ref().join(format!(
        "{}-{}.tgz",
        package.manifest.package.name, package.manifest.package.version
    ));

    if !dry_run {
        std::fs::write(path, package.tgz).map_err(PackageError::Io)?;
    }

    Ok(())
}

#[derive(Error, Debug)]
pub enum PublishError {
    #[cfg(feature = "build")]
    #[error("Failed to get repository status: {0}")]
    RepositoryStatus(git2::Error),
    #[cfg(feature = "build")]
    #[error("Cannot publish dirty repository")]
    DirtyRepository,
    #[error("Cannot release package. {0}")]
    Release(package::ReleaseError),
    #[error("Cannot instance registry client. {0}")]
    Registry(registry::artifactory::BuildError),
    #[error(transparent)]
    Internal(registry::PublishError),
}

/// Publishes the api package to the registry
pub async fn publish(
    credentials: Credentials,
    registry: RegistryUri,
    repository: String,
    #[cfg(feature = "build")] allow_dirty: bool,
    dry_run: bool,
) -> Result<(), PublishError> {
    #[cfg(feature = "build")]
    if let Ok(repository) = git2::Repository::discover(Path::new(".")) {
        let statuses = repository
            .statuses(None)
            .map_err(PublishError::RepositoryStatus)?;

        if !allow_dirty && !statuses.is_empty() {
            tracing::error!("{} files in the working directory contain changes that were not yet committed into git:\n", statuses.len());

            statuses
                .iter()
                .for_each(|s| tracing::error!("{}", s.path().unwrap_or_default()));

            tracing::error!("\nTo proceed with publishing despite the uncommitted changes, pass the `--allow-dirty` flag\n");

            return Err(PublishError::DirtyRepository);
        }
    }

    let artifactory = Artifactory::new(registry, &credentials).map_err(PublishError::Registry)?;

    let package = PackageStore::release()
        .await
        .map_err(PublishError::Release)?;

    if dry_run {
        tracing::warn!(":: aborting upload due to dry run");
        return Ok(());
    }

    artifactory
        .publish(package, repository)
        .await
        .map_err(PublishError::Internal)?;

    Ok(())
}

#[derive(Error, Debug)]
pub enum InstallError {
    #[error("Failed to read manifest. {0}")]
    ReadManifest(manifest::ReadError),
    #[error("Failed to read lockfile. {0}")]
    ReadLockfile(lock::ReadError),
    #[error("Dependency resolution failed. {0}")]
    BuildDependencyGraph(package::DependencyGraphBuildError),
    #[error("Failed to write lockfile. {0}")]
    WriteLockfile(lock::WriteError),
    #[error("Failed to unpack dependency. {0}")]
    UnpackError(package::UnpackError),
}

/// Installs dependencies
pub async fn install(credentials: Credentials) -> Result<(), InstallError> {
    let credentials = Arc::new(credentials);

    let manifest = Manifest::read().await.map_err(InstallError::ReadManifest)?;

    let lockfile = Lockfile::read_or_default()
        .await
        .map_err(InstallError::ReadLockfile)?;

    let dependency_graph = DependencyGraph::from_manifest(&manifest, &lockfile, &credentials)
        .await
        .map_err(InstallError::BuildDependencyGraph)?;

    let mut locked = Vec::new();

    #[async_recursion]
    async fn traverse_and_install(
        name: &PackageName,
        graph: &DependencyGraph,
        locked: &mut Vec<LockedPackage>,
        prefix: String,
    ) -> Result<(), InstallError> {
        let resolved = graph
            .get(name)
            .expect("Unexpected error: missing dependency in dependency graph");

        PackageStore::unpack(&resolved.package)
            .await
            .map_err(InstallError::UnpackError)?;

        tracing::info!(
            "{} installed {}@{}",
            if prefix.is_empty() { "::" } else { &prefix },
            name,
            resolved.package.version()
        );

        locked.push(resolved.package.lock(
            resolved.registry.clone(),
            resolved.repository.clone(),
            resolved.dependants.len(),
        ));

        for (index, dependency) in resolved.depends_on.iter().enumerate() {
            let tree_char = if index + 1 == resolved.depends_on.len() {
                '┗'
            } else {
                '┣'
            };

            let new_prefix = format!(
                "{} {tree_char}",
                if prefix.is_empty() { "  " } else { &prefix }
            );
            traverse_and_install(dependency, graph, locked, new_prefix).await?;
        }

        Ok(())
    }

    for dependency in manifest.dependencies {
        traverse_and_install(
            &dependency.package,
            &dependency_graph,
            &mut locked,
            String::new(),
        )
        .await?;
    }

    let lockfile = Lockfile::from_iter(locked.into_iter());
    lockfile
        .write()
        .await
        .map_err(InstallError::WriteLockfile)?;

    Ok(())
}

/// Uninstalls dependencies
pub async fn uninstall() -> Result<(), IoError> {
    PackageStore::clear().await
}

/// Generate bindings for a given language
#[cfg(feature = "build")]
pub async fn generate(language: Language) -> eyre::Result<()> {
    use eyre::Context; // temporary

    generator::generate(language)
        .await
        .wrap_err_with(|| format!("Failed to generate language bindings for {language}"))?;

    Ok(())
}

#[derive(Error, Debug)]
pub enum LoginError {
    #[error("Failed to read token: {0}")]
    Input(std::io::Error),
    #[error("Failed to instantiate an Artifactory client. {0}")]
    Registry(registry::artifactory::BuildError),
    #[error("Failed to reach artifactory. Please make sure the URL and credentials are correct and the instance is up and running")]
    Ping(registry::artifactory::PingError),
    #[error("Failed to update credentials: {0}")]
    Write(credentials::WriteError),
}

/// Logs you in for a registry
pub async fn login(mut credentials: Credentials, registry: RegistryUri) -> Result<(), LoginError> {
    let token = {
        tracing::info!("Please enter your artifactory token:");

        let mut raw = String::new();

        std::io::stdin()
            .read_line(&mut raw)
            .map_err(LoginError::Input)?;

        raw.trim().into()
    };

    credentials.registry_tokens.insert(registry.clone(), token);

    if env::var("BUFFRS_TESTSUITE").is_err() {
        Artifactory::new(registry, &credentials)
            .map_err(LoginError::Registry)?
            .ping()
            .await
            .map_err(LoginError::Ping)?;
    }

    credentials.write().await.map_err(LoginError::Write)
}

#[derive(Error, Debug)]
pub enum LogoutError {
    #[error("Failed to update credentials: {0}")]
    Write(credentials::WriteError),
}

/// Logs you out from a registry
pub async fn logout(
    mut credentials: Credentials,
    registry: RegistryUri,
) -> Result<(), LogoutError> {
    credentials.registry_tokens.remove(&registry);
    credentials.write().await.map_err(LogoutError::Write)
}
