// (c) Copyright 2023 Helsing GmbH. All rights reserved.

#![doc = include_str!("../README.md")]

use std::path::{Path, PathBuf};

pub use generator::Language;

/// Credential management
pub mod credentials;
/// Code generator
pub mod generator;
/// Manifest format and IO
pub mod manifest;
/// Packages formats and utilities
pub mod package;
/// Supported registries
pub mod registry;

/// Cargo build integration for buffrs
///
/// Important: Only use this inside of cargo build scripts!
pub fn build(language: Language, base_dir: &PathBuf) -> eyre::Result<()> {
    use credentials::Credentials;
    use eyre::ContextCompat;
    use eyre::WrapErr;
    use manifest::Manifest;
    use package::PackageStore;
    use registry::Artifactory;

    let package_store = PackageStore {
        base_dir: base_dir.to_path_buf(),
    };
    println!(
        "cargo:rerun-if-changed={}",
        package_store.proto_vendor_path().display()
    );

    async fn install(base_dir: &Path) -> eyre::Result<()> {
        let credentials = Credentials::read().await?;

        let artifactory = Artifactory::from(
            credentials
                .artifactory
                .wrap_err("Artifactory configuration is required")?,
        );

        let manifest = Manifest::read(base_dir).await?;

        let package_store = PackageStore {
            base_dir: base_dir.to_path_buf(),
        };
        let mut install = Vec::new();

        for dependency in manifest.dependencies {
            if let Ok(pkg) = package_store.resolve(&dependency.package).await {
                let pkg = pkg.package.wrap_err_with(|| {
                    format!(
                        "required package entry in manifest of {} to be present",
                        dependency.package
                    )
                })?;

                if dependency.manifest.version.matches(&pkg.version) {
                    continue;
                }
            }

            install.push(package_store.install(dependency, artifactory.clone()));
        }

        futures::future::try_join_all(install)
            .await
            .wrap_err("Failed to install missing dependencies")?;

        Ok(())
    }

    let rt = tokio::runtime::Runtime::new()?;

    rt.block_on(install(base_dir))?;
    rt.block_on(generator::generate(language, base_dir))?;

    Ok(())
}
