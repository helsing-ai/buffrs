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
use crate::{generator, generator::Language};
#[cfg(feature = "build")]
use std::path::PathBuf;

use async_recursion::async_recursion;
use semver::{Version, VersionReq};
use std::{env, path::Path, sync::Arc};
use thiserror::Error;

const INITIAL_VERSION: Version = Version::new(0, 1, 0);

/// Error produced when initializing a Buffrs project
#[derive(Error, Debug)]
pub enum InitError {
    /// Repository was already initialized
    #[error("A manifest file is already present in the current directory and won't be overridden")]
    ManifestExists,
    /// Failed to interface with filesystem
    #[error("{0}")]
    Io(IoError),
    /// Failed to write the manifest to the filesystem
    #[error("Failed to write manifest. Cause: {0}")]
    ManifestWrite(manifest::WriteError),
    /// System path contains non-unicode data
    #[error("Path is not valid a UTF-8 string")]
    Utf8Error,
    /// Package name contains invalid characters
    #[error("Invalid package name. {0}")]
    PackageNameValidation(PackageNameValidationError),
}

/// Initializes the project
pub async fn init(kind: PackageType, name: Option<PackageName>) -> Result<(), InitError> {
    if Manifest::exists().await.map_err(InitError::Io)? {
        return Err(InitError::ManifestExists);
    }

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

/// Error produced by the add command
#[derive(Error, Debug)]
pub enum AddError {
    /// A repository component could not be extracted
    #[error("Locator {0} is missing a repository delimiter")]
    MissingRepository(String),
    /// Repository name not in kebab case
    #[error("Repository {0} is not in kebab case")]
    NotKebabCase(String),
    /// No version could be extracted
    #[error("Dependency specification is missing version part: {0}")]
    MissingVersion(String),
    /// Dependency name contains invalid characters
    #[error("Failed to validate dependency name. {0}")]
    InvalidName(package::PackageNameValidationError),
    /// Version does not follow SemVer spec
    #[error("Not a valid version requirement: {given}. Cause: {source}")]
    InvalidVersionReq {
        /// The raw specification provided by the user
        given: String,
        /// The original validation error produced by the semver crate
        source: semver::Error,
    },
    /// Manifest file could not be read
    #[error("Failed to read manifest. {0}")]
    ManifestRead(manifest::ReadError),
    /// Manifest file could not be written to
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

/// Error produced by the remove command
#[derive(Error, Debug)]
pub enum RemoveError {
    /// Failed to read the manifest file
    #[error("Failed to read manifest. {0}")]
    ManifestRead(manifest::ReadError),
    /// Package not present in manifest
    #[error("Package {0} not in manifest")]
    PackageNotFound(PackageName),
    /// Failed to write to the manifest file
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

/// Error produced by the package command
#[derive(Error, Debug)]
pub enum PackageError {
    /// Could not generate a package release from the project
    #[error("Failed to release package. {0}")]
    Release(package::ReleaseError),
    /// Could not write the package to the filesystem
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

/// Error produced by the publish command
#[derive(Error, Debug)]
pub enum PublishError {
    /// Failed to fetch repository status from git
    #[cfg(feature = "build")]
    #[error("Failed to get repository status: {0}")]
    RepositoryStatus(git2::Error),
    /// Attempted to publish a dirty repository
    #[cfg(feature = "build")]
    #[error("Cannot publish dirty repository")]
    DirtyRepository,
    /// Failed to generate a package release
    #[error("Cannot release package. {0}")]
    Release(package::ReleaseError),
    /// Failed to instantiate the registry client
    #[error("Cannot instance registry client. {0}")]
    Registry(registry::artifactory::BuildError),
    /// Some other error produced by the registry client
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

/// Error produced by the install command
#[derive(Error, Debug)]
pub enum InstallError {
    /// Could not read the manifest file
    #[error("Failed to read manifest. {0}")]
    ReadManifest(manifest::ReadError),
    /// Could not read the lockfile
    #[error("Failed to read lockfile. {0}")]
    ReadLockfile(lock::ReadError),
    /// Could not generate the dependency graph
    #[error("Dependency resolution failed. {0}")]
    BuildDependencyGraph(package::DependencyGraphBuildError),
    /// Could not write to the lockfile
    #[error("Failed to write lockfile. {0}")]
    WriteLockfile(lock::WriteError),
    /// Could not extract the package to the local fs store
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

/// Error produced by the generate command
#[derive(Error, Debug)]
#[error("Failed to generate language bindings for {language}. Cause: {source}")]
pub struct GenerateError {
    source: generator::GenerateError,
    language: Language,
}

/// Generate bindings for a given language
#[cfg(feature = "build")]
pub async fn generate(language: Language, out_dir: PathBuf) -> Result<(), GenerateError> {
    generator::Generator::Protoc { language, out_dir }
        .generate()
        .await
        .map_err(|err| GenerateError {
            source: err,
            language,
        })
}

/// Error produced by the login command
#[derive(Error, Debug)]
pub enum LoginError {
    /// Failed to read the token from the user
    #[error("Failed to read token: {0}")]
    Input(std::io::Error),
    /// Failed to instantiate the registry client
    #[error("Failed to instantiate an Artifactory client. {0}")]
    Registry(registry::artifactory::BuildError),
    /// Failed to reach the registry
    #[error("Failed to reach artifactory. Please make sure the URL and credentials are correct and the instance is up and running")]
    Ping(registry::artifactory::PingError),
    /// Failed to write to the credentials file
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

/// Error produced by the logout command
#[derive(Error, Debug)]
pub enum LogoutError {
    /// Failed to write to the credentials file
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
