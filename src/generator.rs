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

use std::{fmt, path::Path};

use eyre::Context;
use protoc_bin_vendored::protoc_bin_path;
use serde::{Deserialize, Serialize};

use crate::{manifest::Manifest, package::PackageStore};

/// The directory used for the generated code
pub const BUILD_DIRECTORY: &str = "proto/build";

/// The language used for code generation
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, clap::ValueEnum,
)]
#[serde(rename_all = "kebab-case")]
pub enum Language {
    /// Rust language
    Rust,
}

impl fmt::Display for Language {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", serde_typename::to_str(&self).unwrap_or("unknown"))
    }
}

/// Backend used to generate code bindings
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Generator {
    /// The tonic + prost stack
    Tonic,
}

impl Generator {
    /// Tonic include file name
    pub const TONIC_INCLUDE_FILE: &str = "buffrs.rs";

    /// Run the generator for a dependency and output files at the provided path
    pub async fn run(&self) -> eyre::Result<()> {
        let protoc = protoc_bin_path().wrap_err("Unable to locate vendored protoc")?;

        std::env::set_var("PROTOC", protoc.clone());

        let store = Path::new(PackageStore::PROTO_PATH);
        let protos = PackageStore::collect(store).await;
        let includes = &[store];

        match self {
            Generator::Tonic => {
                tonic_build::configure()
                    .build_client(true)
                    .build_server(true)
                    .build_transport(true)
                    .include_file(Self::TONIC_INCLUDE_FILE)
                    .compile(&protos, includes)?;
            }
        }

        Ok(())
    }
}

/// Generate the code bindings for a language
pub async fn generate(language: Language) -> eyre::Result<()> {
    let manifest = Manifest::read().await?;

    tracing::info!(":: initializing code generator for {language}");

    eyre::ensure!(
        manifest.package.kind.compilable() || !manifest.dependencies.is_empty(),
        "Either a compilable package (library or api) or at least one dependency is needed to generate code bindings."
    );

    // Only tonic is supported right now
    let generator = Generator::Tonic;

    generator
        .run()
        .await
        .wrap_err_with(|| format!("Failed to generate bindings for {language}"))?;

    if manifest.package.kind.compilable() {
        let location = Path::new(PackageStore::PROTO_PATH);
        tracing::info!(
            ":: compiled {} [{}]",
            manifest.package.name,
            location.display()
        );
    }

    for dependency in manifest.dependencies {
        let location = PackageStore::locate(&dependency.package);
        tracing::info!(
            ":: compiled {} [{}]",
            dependency.package,
            location.display()
        );
    }

    Ok(())
}
