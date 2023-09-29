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

#![warn(missing_docs)]
#![doc = include_str!("../README.md")]

/// CLI command implementations
pub mod command;
/// Credential management
pub mod credentials;
/// Common error types
pub mod errors;
/// Code generator
#[cfg(feature = "build")]
pub mod generator;
/// Lockfile implementation
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
    use eyre::Context;
    use package::PackageStore;

    async fn install() -> eyre::Result<()> {
        let credentials = Credentials::read().await?;
        command::install(credentials)
            .await
            .wrap_err("Installation failed.")
    }

    println!("cargo:rerun-if-changed={}", PackageStore::PROTO_VENDOR_PATH);

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
