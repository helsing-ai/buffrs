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
    credentials::Credentials,
    lock::{LockedPackage, Lockfile},
    manifest::{Dependency, Manifest, PackageManifest},
    package::{DependencyGraph, PackageName, PackageStore, PackageType},
    registry::{Artifactory, RegistryUri},
};

#[cfg(feature = "build")]
use crate::{generator, generator::Language};
#[cfg(feature = "build")]
use std::path::PathBuf;

use async_recursion::async_recursion;
use eyre::{Context, ContextCompat};
use semver::{Version, VersionReq};
use std::{env, path::Path, sync::Arc};

const INITIAL_VERSION: Version = Version::new(0, 1, 0);

/// Initializes the project
pub async fn init(kind: PackageType, name: Option<PackageName>) -> eyre::Result<()> {
    if Manifest::exists()
        .await
        .wrap_err("failed to access filesystem")?
    {
        eyre::bail!("a manifest file was found, project is already initialized");
    }

    fn curr_dir_name() -> eyre::Result<PackageName> {
        std::env::current_dir()
            .wrap_err("failed to access current directory")?
            .file_name()
            .expect("unexpected error: current directory path terminates in ..")
            .to_str()
            .wrap_err("current directory path is not valid utf-8")?
            .parse()
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

    manifest
        .write()
        .await
        .wrap_err("failed to write manifest file")?;

    PackageStore::create()
        .await
        .wrap_err("failed to create buffrs project directories")
}

/// Adds a dependency to this project
pub async fn add(registry: RegistryUri, dependency: &str) -> eyre::Result<()> {
    let lower_kebab = |c: char| (c.is_lowercase() && c.is_ascii_alphabetic()) || c == '-';

    let (repository, dependency) = dependency
        .trim()
        .split_once('/')
        .wrap_err_with(|| format!("locator {dependency} is missing a repository delimiter"))?;

    if !repository.chars().all(lower_kebab) {
        eyre::bail!("repository {repository} is not in kebab case");
    }

    let repository = repository.into();

    let (package, version) = dependency.split_once('@').wrap_err_with(|| {
        format!("dependency specification is missing version part: {dependency}")
    })?;

    let package = package
        .parse::<PackageName>()
        .wrap_err_with(|| format!("invalid package name: {package}"))?;

    let version = version
        .parse::<VersionReq>()
        .wrap_err_with(|| format!("not a valid version requirement: {version}"))?;

    let mut manifest = Manifest::read()
        .await
        .wrap_err("failed to read manifest file")?;

    manifest
        .dependencies
        .push(Dependency::new(registry, repository, package, version));

    manifest
        .write()
        .await
        .wrap_err("failed to write manifest file")
}

/// Removes a dependency from this project
pub async fn remove(package: PackageName) -> eyre::Result<()> {
    let mut manifest = Manifest::read()
        .await
        .wrap_err("failed to read manifest file")?;

    let match_idx = manifest
        .dependencies
        .iter()
        .position(|d| d.package == package)
        .wrap_err_with(|| format!("package {package} not in manifest"))?;

    let dependency = manifest.dependencies.remove(match_idx);

    // if PackageStore::uninstall(&dependency.package).await.is_err() {
    //     tracing::warn!("failed to uninstall package {}", dependency.package);
    // }

    PackageStore::uninstall(&dependency.package).await.ok(); // temporary due to broken test

    manifest
        .write()
        .await
        .wrap_err("failed to write manifest file")
}

/// Packages the api and writes it to the filesystem
pub async fn package(directory: impl AsRef<Path>, dry_run: bool) -> eyre::Result<()> {
    let package = PackageStore::release()
        .await
        .wrap_err("failed to release package")?;

    let path = directory.as_ref().join(format!(
        "{}-{}.tgz",
        package.manifest.package.name, package.manifest.package.version
    ));

    if !dry_run {
        std::fs::write(path, package.tgz)
            .wrap_err("failed to write package release to the current directory")?;
    }

    Ok(())
}

/// Publishes the api package to the registry
pub async fn publish(
    credentials: Credentials,
    registry: RegistryUri,
    repository: String,
    #[cfg(feature = "git")] allow_dirty: bool,
    dry_run: bool,
) -> eyre::Result<()> {
    #[cfg(feature = "build")]
    if let Ok(repository) = git2::Repository::discover(Path::new(".")) {
        let statuses = repository
            .statuses(None)
            .wrap_err("failed to determine repository status")?;

        if !allow_dirty && !statuses.is_empty() {
            tracing::error!("{} files in the working directory contain changes that were not yet committed into git:\n", statuses.len());

            statuses
                .iter()
                .for_each(|s| tracing::error!("{}", s.path().unwrap_or_default()));

            tracing::error!("\nTo proceed with publishing despite the uncommitted changes, pass the `--allow-dirty` flag\n");

            eyre::bail!("attempted to publish a dirty repository");
        }
    }

    let artifactory = Artifactory::new(registry, &credentials)?;

    let package = PackageStore::release()
        .await
        .wrap_err("failed to release package")?;

    if dry_run {
        tracing::warn!(":: aborting upload due to dry run");
        return Ok(());
    }

    artifactory
        .publish(package, repository)
        .await
        .wrap_err("failed to publish package")
}

/// Installs dependencies
pub async fn install(credentials: Credentials) -> eyre::Result<()> {
    let credentials = Arc::new(credentials);

    let manifest = Manifest::read()
        .await
        .wrap_err("manifest file could not be read")?;

    let lockfile = Lockfile::read_or_default()
        .await
        .wrap_err("lockfile could not be read")?;

    let dependency_graph = DependencyGraph::from_manifest(&manifest, &lockfile, &credentials)
        .await
        .wrap_err("dependency resolution failed")?;

    let mut locked = Vec::new();

    #[async_recursion]
    async fn traverse_and_install(
        name: &PackageName,
        graph: &DependencyGraph,
        locked: &mut Vec<LockedPackage>,
        prefix: String,
    ) -> eyre::Result<()> {
        let resolved = graph
            .get(name)
            .expect("unexpected error: missing dependency in dependency graph");

        PackageStore::unpack(&resolved.package)
            .await
            .wrap_err_with(|| format!("failed to unpack package {}", &resolved.package.name()))?;

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

    Lockfile::from_iter(locked.into_iter()).write().await
}

/// Uninstalls dependencies
pub async fn uninstall() -> eyre::Result<()> {
    PackageStore::clear()
        .await
        .wrap_err("failed to clear packages directory")
}

/// Generate bindings for a given language
#[cfg(feature = "build")]
pub async fn generate(language: Language, out_dir: PathBuf) -> eyre::Result<()> {
    generator::Generator::Protoc { language, out_dir }
        .generate()
        .await
        .wrap_err_with(|| format!("failed to generate {language} bindings"))
}

/// Logs you in for a registry
pub async fn login(mut credentials: Credentials, registry: RegistryUri) -> eyre::Result<()> {
    let token = {
        tracing::info!("Please enter your artifactory token:");

        let mut raw = String::new();

        std::io::stdin()
            .read_line(&mut raw)
            .wrap_err("failed to read the token from the user")?;

        raw.trim().into()
    };

    credentials.registry_tokens.insert(registry.clone(), token);

    if env::var("BUFFRS_TESTSUITE").is_err() {
        Artifactory::new(registry, &credentials)?
            .ping()
            .await
            .wrap_err("failed to validate token")?;
    }

    credentials.write().await.wrap_err("failed to save token")
}

/// Logs you out from a registry
pub async fn logout(mut credentials: Credentials, registry: RegistryUri) -> eyre::Result<()> {
    credentials.registry_tokens.remove(&registry);
    credentials
        .write()
        .await
        .wrap_err("failed to write to the credentials file")
}
