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

use std::{
    fmt,
    path::{Path, PathBuf},
};

use eyre::{ensure, Context, ContextCompat};
use protoc_bin_vendored::protoc_bin_path;
use serde::{Deserialize, Serialize};

use crate::{manifest::Manifest, package::PackageStore};

/// The language used for code generation
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, clap::ValueEnum,
)]
#[serde(rename_all = "kebab-case")]
pub enum Language {
    Python,
}

impl fmt::Display for Language {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", serde_typename::to_str(&self).unwrap_or("unknown"))
    }
}

/// Backend used to generate code bindings
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Generator {
    /// The tonic + prost stack
    Tonic,
    /// The official `protoc` protobuf compiler
    Protoc {
        language: Language,
        out_dir: PathBuf,
    },
}

impl Generator {
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
            Generator::Protoc { language, out_dir } => {
                let mut protoc_cmd = tokio::process::Command::new(protoc);

                match language {
                    Language::Python => {
                        protoc_cmd.arg("--python_out").arg(out_dir);
                    }
                }

                // Setting proto path causes protoc to replace occurrences of this string appearing in the
                // path of the generated path with that provided by output path
                // e.g. if input proto path is proto/vendor/units/units.proto and the proto path is 'proto'
                // and the --python_out is 'proto/build/gen' then the file will be output to
                // proto/build/gen/vendor/units/units.py

                protoc_cmd.arg("--proto_path").arg("proto/vendor"); // We need both of these if we want "vendor" to be removed, and it has to come first
                protoc_cmd.arg("--proto_path").arg("proto");

                protoc_cmd.args(&protos);

                tracing::debug!(":: running {protoc_cmd:?}");

                let output = protoc_cmd
                    .output()
                    .await
                    .wrap_err("failed to retrieve protoc output")?;

                let exit = output
                    .status
                    .code()
                    .wrap_err("failed to retrieve exit code of protoc")?;

                ensure!(
                    exit == 0,
                    "protoc generation failed: {}",
                    String::from_utf8(output.stderr)?,
                );

                tracing::info!(":: {language} code generated successfully");
            }
        }

        Ok(())
    }

    pub async fn generate(&self) -> eyre::Result<()> {
        let manifest = Manifest::read().await?;

        tracing::info!(":: initializing code generator");

        eyre::ensure!(
            manifest.package.kind.compilable() || !manifest.dependencies.is_empty(),
            "Either a compilable package (library or api) or at least one dependency is needed to generate code bindings."
        );

        self.run()
            .await
            .wrap_err_with(|| "Failed to generate bindings")?;

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
}
