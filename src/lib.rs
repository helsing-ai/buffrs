// (c) Copyright 2023 Helsing GmbH. All rights reserved.

#![doc = include_str!("../README.md")]

/// Credential management
pub mod credentials;
/// Code generator
#[cfg(feature = "build")]
pub mod generator;
/// Manifest format and IO
pub mod manifest;
/// Packages formats and utilities
pub mod package;
/// Supported registries
pub mod registry;

use std::sync::Arc;

#[cfg(feature = "build")]
pub use generator::Language;

use credentials::Credentials;
use eyre::{ContextCompat, WrapErr};
use manifest::Manifest;
use package::PackageStore;
use registry::Artifactory;

/// Cargo build integration for buffrs
///
/// Important: Only use this inside of cargo build scripts!
#[cfg(feature = "build")]
pub fn build() -> eyre::Result<()> {
    println!("cargo:rerun-if-changed={}", PackageStore::PROTO_VENDOR_PATH);

    let rt = tokio::runtime::Runtime::new()?;

    rt.block_on(install())?;
    rt.block_on(generator::generate(
        Language::Rust,
        std::env::var("OUT_DIR").unwrap(),
    ))?;

    Ok(())
}

async fn install() -> eyre::Result<()> {
    let credentials = Arc::new(Credentials::read().await?);
    let manifest = Manifest::read().await?;

    let mut install = Vec::new();

    for dependency in manifest.dependencies {
        if let Ok(pkg) = PackageStore::resolve(&dependency.package).await {
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

        let artifactory =
            Artifactory::new(credentials.clone(), dependency.manifest.registry.clone());

        install.push(PackageStore::install(
            dependency.clone(),
            artifactory.clone(),
        ));
    }

    futures::future::try_join_all(install)
        .await
        .wrap_err("Failed to install missing dependencies")?;

    Ok(())
}

/// Include generated rust language bindings for buffrs.
///
/// ```rust,ignore
/// mod protos {
///     buffrs::include!();
/// }
/// ```
#[macro_export]
macro_rules! include {
    () => {
        let output_directory = match env!("OUT_DIR") {
            Some(dir) => dir,
            None => {
                let output_directory = "proto/gen".to_string();
                tracing::warn!("outputting to default location: {output_directory}");
                output_directory
            }
        };

        ::std::include!(concat!(output_directory, "/buffrs.rs",));
    };
}
