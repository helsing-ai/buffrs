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

use async_recursion::async_recursion;
use eyre::ContextCompat;

use crate::{
    credentials::Credentials,
    generator,
    lock::{LockedPackage, Lockfile},
    manifest::{Dependency, Manifest, PackageManifest},
    package::{DependencyGraph, PackageName, PackageStore, PackageType},
    registry::{Artifactory, Registry, RegistryUri},
    Language,
};

use std::{env, path::Path, sync::Arc};

use eyre::{ensure, Context};
use semver::{Version, VersionReq};

/// Initializes the project
pub async fn init(r#type: PackageType, name: Option<PackageName>) -> eyre::Result<()> {
    ensure!(
        !Manifest::exists().await?,
        "Cant initialize existing project"
    );

    const DIR_ERR: &str = "Failed to read current directory name";

    let name = match name {
        Some(name) => name,
        None => std::env::current_dir()?
            .file_name()
            .wrap_err(DIR_ERR)?
            .to_str()
            .wrap_err(DIR_ERR)?
            .parse()?,
    };

    const VERSION: Version = Version::new(0, 1, 0);

    let manifest = Manifest {
        package: PackageManifest {
            r#type,
            name,
            version: VERSION,
            description: None,
        },
        dependencies: vec![],
    };

    manifest.write().await?;

    PackageStore::create().await
}

/// Adds a dependency to this project
pub async fn add(registry: RegistryUri, dependency: String) -> eyre::Result<()> {
    let lower_kebab = |c: char| (c.is_lowercase() && c.is_ascii_alphabetic()) || c == '-';

    let (repository, dependency) = dependency
        .trim()
        .split_once('/')
        .wrap_err("Invalid dependency specification")?;

    ensure!(
        repository.chars().all(lower_kebab),
        "Repositories must be in lower kebab case"
    );

    let repository = repository.into();

    let (package, version) = dependency
        .split_once('@')
        .wrap_err_with(|| format!("Invalid dependency specification: {dependency}"))?;

    let package = package
        .parse::<PackageName>()
        .wrap_err_with(|| format!("Invalid package name supplied: {package}"))?;

    let version = version
        .parse::<VersionReq>()
        .wrap_err_with(|| format!("Invalid version requirement supplied: {package}"))?;

    let mut manifest = Manifest::read().await?;

    manifest
        .dependencies
        .push(Dependency::new(registry, repository, package, version));

    manifest.write().await
}

/// Removes a dependency from this project
pub async fn remove(package: PackageName) -> eyre::Result<()> {
    let mut manifest = Manifest::read().await?;

    let dependency = manifest
        .dependencies
        .iter()
        .find(|d| d.package == package)
        .wrap_err(eyre::eyre!(
            "Unable to remove unknown dependency {package:?}"
        ))?
        .to_owned();

    manifest.dependencies.retain(|d| *d != dependency);

    PackageStore::uninstall(&dependency.package).await.ok();

    manifest.write().await
}

/// Packages the api and writes it to the filesystem
pub async fn package(directory: String, dry_run: bool) -> eyre::Result<()> {
    let package = PackageStore::release()
        .await
        .wrap_err("Failed to create release")?;

    let path = Path::new(&directory).join(format!(
        "{}-{}.tgz",
        package.manifest.package.name, package.manifest.package.version
    ));

    if !dry_run {
        std::fs::write(path, package.tgz).wrap_err("failed to write package to filesystem")?;
    }

    Ok(())
}

/// Publishes the api package to the registry
pub async fn publish(
    credentials: Credentials,
    registry: RegistryUri,
    repository: String,
    allow_dirty: bool,
    dry_run: bool,
) -> eyre::Result<()> {
    if let Ok(repository) = git2::Repository::discover(Path::new(".")) {
        let statuses = repository
            .statuses(None)
            .wrap_err("Failed to get git status")?;

        if !allow_dirty && !statuses.is_empty() {
            tracing::error!("{} files in the working directory contain changes that were not yet committed into git:\n", statuses.len());

            statuses
                .iter()
                .for_each(|s| tracing::error!("{}", s.path().unwrap_or_default()));

            tracing::error!("\nTo proceed with publishing despite the uncommitted changes, pass the `--allow-dirty` flag\n");

            eyre::bail!("Unable to publish a dirty git repository");
        }
    }

    let artifactory = Artifactory::new(Arc::new(credentials), registry);

    let package = PackageStore::release()
        .await
        .wrap_err("Failed to create release")?;

    if dry_run {
        tracing::warn!(":: aborting upload due to dry run");
        return Ok(());
    }

    artifactory.publish(package, repository).await?;

    Ok(())
}

/// Installs dependencies
pub async fn install(credentials: Credentials) -> eyre::Result<()> {
    let credentials = Arc::new(credentials);

    let manifest = Manifest::read().await?;

    let lockfile = Lockfile::read_or_default().await?;

    let dependency_tree =
        DependencyGraph::from_manifest(&manifest, &lockfile, &credentials).await?;

    let mut locked = Vec::new();

    #[async_recursion]
    async fn process_recursively(
        name: &PackageName,
        tree: &DependencyGraph,
        locked: &mut Vec<LockedPackage>,
        prefix: String,
    ) -> eyre::Result<()> {
        let resolved = tree
            .get(name)
            .wrap_err("Dependency missing from dependency tree")?;

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
            process_recursively(dependency, tree, locked, new_prefix).await?;
        }

        Ok(())
    }

    for dependency in manifest.dependencies {
        process_recursively(
            &dependency.package,
            &dependency_tree,
            &mut locked,
            String::new(),
        )
        .await?;
    }

    let lockfile = Lockfile::from_iter(locked.into_iter());
    lockfile.write().await?;

    Ok(())
}

/// Uninstalls dependencies
pub async fn uninstall() -> eyre::Result<()> {
    PackageStore::clear().await
}

/// Generate bindings for a given language
pub async fn generate(language: Language) -> eyre::Result<()> {
    generator::generate(language)
        .await
        .wrap_err_with(|| format!("Failed to generate language bindings for {language}"))?;

    Ok(())
}

/// Logs you in for a registry
pub async fn login(mut credentials: Credentials, registry: RegistryUri) -> eyre::Result<()> {
    let token = {
        tracing::info!("Please enter your artifactory token:");

        let mut raw = String::new();

        std::io::stdin()
            .read_line(&mut raw)
            .wrap_err("Failed to read token")?;

        raw.trim().into()
    };

    credentials.registry_tokens.insert(registry.clone(), token);

    let artifactory = Artifactory::new(Arc::new(credentials.clone()), registry.clone());

    if env::var("BUFFRS_TESTSUITE").is_err() {
        artifactory
            .ping()
            .await
            .wrap_err("Failed to reach artifactory, please make sure the url and credentials are correct and the instance is up and running")?;
    }

    credentials.write().await
}

/// Logs you out from a registry
pub async fn logout(mut credentials: Credentials, registry: RegistryUri) -> eyre::Result<()> {
    credentials.registry_tokens.remove(&registry);
    credentials.write().await
}
