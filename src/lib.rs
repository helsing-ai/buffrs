// (c) Copyright 2023 Helsing GmbH. All rights reserved.

#![doc = include_str!("../README.md")]

/// CLI command implementations
pub mod command;
/// Credential management
pub mod credentials;
/// Code generator
#[cfg(feature = "build")]
pub mod generator;
/// Lockfile Implementation
pub mod lock;
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
    use package::PackageStore;

    println!("cargo:rerun-if-changed={}", PackageStore::PROTO_VENDOR_PATH);

    async fn install() -> eyre::Result<()> {
        let credentials = Credentials::read().await?;

        command::install(credentials).await?;

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
