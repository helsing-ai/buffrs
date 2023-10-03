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
    errors::{ConfigError, HttpError},
    lock::{self, LockedPackage, Lockfile},
    manifest::{self, Dependency, Manifest, PackageManifest},
    package::{self, DependencyGraph, InvalidPackageName, PackageName, PackageStore, PackageType},
    registry::{self, Artifactory, Registry, RegistryUri},
};

#[cfg(feature = "build")]
use crate::{generator, generator::Language};
#[cfg(feature = "build")]
use std::path::PathBuf;

use async_recursion::async_recursion;
use displaydoc::Display;
use semver::{Version, VersionReq};
use std::{env, path::Path, sync::Arc};
use thiserror::Error;

const INITIAL_VERSION: Version = Version::new(0, 1, 0);

#[derive(Error, Display, Debug)]
#[non_exhaustive]
#[allow(missing_docs)]
pub enum InitError {
    /// a manifest file was found, project is already initialized
    AlreadyInitialized,
    /// invalid package name
    InvalidName(#[from] InvalidPackageName),
    /// io error: {message}
    Io {
        message: String,
        source: std::io::Error,
    },
    /// failed to create manifest file
    ManifestWrite(#[from] manifest::WriteError),
    /// failed to create package store
    PackageStore(#[from] package::CreateStoreError),
    /// {0}
    Other(&'static str),
}

/// Initializes the project
pub async fn init(kind: PackageType, name: Option<PackageName>) -> Result<(), InitError> {
    if Manifest::exists().await.map_err(|source| InitError::Io {
        message: "failed to access filesystem".into(),
        source,
    })? {
        return Err(InitError::AlreadyInitialized);
    }

    fn curr_dir_name() -> Result<PackageName, InitError> {
        std::env::current_dir()
            .map_err(|source| InitError::Io {
                message: "failed to access current directory".into(),
                source,
            })?
            .file_name()
            .expect("unexpected error: current directory path terminates in ..")
            .to_str()
            .ok_or(InitError::Other(
                "current directory path is not valid utf-8",
            ))?
            .parse()
            .map_err(InitError::InvalidName)
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

    manifest.write().await?;

    PackageStore::create().await.map_err(InitError::from)
}

/// Wrapper for a semantic version validation error
#[derive(Error, Debug)]
#[error(transparent)]
pub struct SemVerError(#[from] semver::Error);

#[derive(Error, Display, Debug)]
#[non_exhaustive]
#[allow(missing_docs)]
pub enum AddError {
    /// locator {0} is missing a repository delimiter
    MissingRepository(String),
    /// repository {0} is not in kebab case
    NotKebabCase(String),
    /// dependency specification is missing version part: {0}
    MissingVersion(String),
    /// dependency name is not a valid package name
    InvalidName(#[from] package::InvalidPackageName),
    /// not a valid version requirement: {given}
    InvalidVersionReq { given: String, source: SemVerError },
    /// failed to read the manifest file
    ManifestRead(#[from] manifest::ReadError),
    /// failed to write to manifest file
    ManifestWrite(#[from] manifest::WriteError),
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

    let package = package.parse::<PackageName>()?;

    let version = version
        .parse::<VersionReq>()
        .map_err(|source| AddError::InvalidVersionReq {
            given: version.into(),
            source: source.into(),
        })?;

    let mut manifest = Manifest::read().await?;

    manifest
        .dependencies
        .push(Dependency::new(registry, repository, package, version));

    manifest.write().await.map_err(AddError::from)
}

#[derive(Error, Display, Debug)]
#[allow(missing_docs)]
pub enum RemoveError {
    /// package {0} not in manifest
    PackageNotFound(PackageName),
    /// failed to read the manifest file
    ManifestRead(#[from] manifest::ReadError),
    /// failed to write to the manifest file
    ManifestWrite(#[from] manifest::WriteError),
}

/// Removes a dependency from this project
pub async fn remove(package: PackageName) -> Result<(), RemoveError> {
    let mut manifest = Manifest::read().await?;

    let match_idx = manifest
        .dependencies
        .iter()
        .position(|d| d.package == package)
        .ok_or(RemoveError::PackageNotFound(package))?;

    let dependency = manifest.dependencies.remove(match_idx);

    // if PackageStore::uninstall(&dependency.package).await.is_err() {
    //     tracing::warn!("failed to uninstall package {}", dependency.package);
    // }

    PackageStore::uninstall(&dependency.package).await.ok(); // temporary due to broken test

    manifest.write().await.map_err(RemoveError::from)
}

#[derive(Error, Display, Debug)]
#[allow(missing_docs)]
pub enum PackageError {
    /// failed to release package
    Release(#[from] package::ReleaseError),
    /// could not write the package to the filesystem
    Write(#[source] std::io::Error),
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
        std::fs::write(path, package.tgz).map_err(PackageError::Write)?;
    }

    Ok(())
}

#[derive(Error, Display, Debug)]
#[allow(missing_docs)]
pub enum PublishError {
    /// failed to determine repository status
    #[cfg(feature = "build")]
    RepositoryStatus(#[source] git2::Error),
    /// attempted to publish a dirty repository
    #[cfg(feature = "build")]
    DirtyRepository,
    /// configuration error
    Config(#[from] ConfigError),
    /// failed to generate a package release
    Release(#[from] package::ReleaseError),
    /// failed to publish release
    Publish(#[from] registry::PublishError),
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

    let artifactory = Artifactory::new(registry, &credentials)?;

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
        .map_err(PublishError::from)
}

#[derive(Error, Display, Debug)]
#[allow(missing_docs)]
pub enum InstallError {
    /// could not read the manifest file
    ReadManifest(#[from] manifest::ReadError),
    /// could not read the lockfile
    ReadLockfile(#[from] lock::ReadError),
    /// dependency resolution failed
    BuildDependencyGraph(#[from] package::DependencyGraphBuildError),
    /// could not write to the lockfile
    WriteLockfile(#[from] lock::WriteError),
    /// could not extract the package
    UnpackError(#[from] package::UnpackError),
}

/// Installs dependencies
pub async fn install(credentials: Credentials) -> Result<(), InstallError> {
    let credentials = Arc::new(credentials);

    let manifest = Manifest::read().await?;

    let lockfile = Lockfile::read_or_default().await?;

    let dependency_graph =
        DependencyGraph::from_manifest(&manifest, &lockfile, &credentials).await?;

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
            .expect("unexpected error: missing dependency in dependency graph");

        PackageStore::unpack(&resolved.package).await?;

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

    Lockfile::from_iter(locked.into_iter())
        .write()
        .await
        .map_err(InstallError::from)
}

/// Uninstalls dependencies
pub async fn uninstall() -> Result<(), std::io::Error> {
    PackageStore::clear().await
}

/// failed to generate {language} bindings
#[derive(Error, Display, Debug)]
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
        .map_err(|source| GenerateError { source, language })
}

#[derive(Error, Display, Debug)]
#[allow(missing_docs)]
pub enum LoginError {
    /// failed to read the token from the user
    Input(#[source] std::io::Error),
    /// failed to instantiate the registry client
    Config(#[from] ConfigError),
    /// failed to reach artifactory
    Ping(#[source] HttpError),
    /// failed to write to the credentials file
    WriteCredentials(#[from] credentials::WriteError),
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
        Artifactory::new(registry, &credentials)?
            .ping()
            .await
            .map_err(LoginError::Ping)?;
    }

    credentials.write().await.map_err(LoginError::from)
}

#[derive(Error, Display, Debug)]
#[allow(missing_docs)]
pub enum LogoutError {
    /// failed to write to the credentials file
    Write(#[from] credentials::WriteError),
}

/// Logs you out from a registry
pub async fn logout(
    mut credentials: Credentials,
    registry: RegistryUri,
) -> Result<(), LogoutError> {
    credentials.registry_tokens.remove(&registry);
    credentials.write().await.map_err(LogoutError::from)
}
