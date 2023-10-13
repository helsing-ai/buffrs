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

mod managed_file;

/// Cargo build integration for buffrs
///
/// Important: Only use this inside of cargo build scripts!
#[cfg(feature = "build")]
#[tokio::main(flavor = "current_thread")]
pub async fn build() -> miette::Result<()> {
    use credentials::Credentials;
    use package::PackageStore;

    println!("cargo:rerun-if-changed={}", PackageStore::PROTO_VENDOR_PATH);

    let credentials = Credentials::read().await?;
    command::install(credentials).await?;

    generator::Generator::Tonic.generate().await?;

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
