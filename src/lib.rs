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

#[cfg(feature = "build")]
pub use generator::Language;

/// Cargo build integration for buffrs
///
/// Important: Only use this inside of cargo build scripts!
#[cfg(feature = "build")]
pub fn build() -> eyre::Result<()> {
    use credentials::Credentials;
    use eyre::ContextCompat;
    use eyre::WrapErr;
    use manifest::Manifest;
    use package::PackageStore;
    use registry::Artifactory;

    println!("cargo:rerun-if-changed={}", PackageStore::PROTO_VENDOR_PATH);

    async fn install() -> eyre::Result<()> {
        let credentials = Credentials::read().await?;

        let artifactory = Artifactory::from(
            credentials
                .artifactory
                .wrap_err("Artifactory configuration is required")?,
        );

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

            install.push(PackageStore::install(dependency, artifactory.clone()));
        }

        futures::future::try_join_all(install)
            .await
            .wrap_err("Failed to install missing dependencies")?;

        Ok(())
    }

    let rt = tokio::runtime::Runtime::new()?;

    rt.block_on(install())?;
    rt.block_on(generator::generate(Language::Rust))?;

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
        ::std::include!(concat!(env!("OUT_DIR"), "/buffrs.rs",));
    };
}
