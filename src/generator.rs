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

use protoc_bin_vendored::protoc_bin_path;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    manifest::{self, Manifest},
    package::PackageStore,
};

/// The language used for code generation
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, clap::ValueEnum,
)]
#[serde(rename_all = "kebab-case")]
pub enum Language {
    /// The Python language
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
        /// Target language for code generation
        language: Language,
        /// Target directory for the generated source files
        out_dir: PathBuf,
    },
}

/// Error produced when running the code generator
#[derive(Error, Debug)]
pub enum RunError {
    /// The protoc binary could not be located in the local filesystem
    #[error("Unable to locate vendored protoc")]
    Locate,
    /// A generic input/output error
    #[error("IO error: {0}")]
    Io(std::io::Error),
    /// Subprocess terminated by process signal
    #[error("A signal interrupted the protoc subprocess before it could complete")]
    Interrupted,
    /// Subprocess returned non-zero exit code indicating error during execution
    #[error("The protoc subprocess terminated with an error: {code}. Error output:\n{}", String::from_utf8_lossy(.stderr))]
    NonZeroExitCode {
        /// The exit code from the subprocess
        code: i32,
        /// The captured output from stderr
        stderr: Vec<u8>,
    },
}

impl Generator {
    /// Tonic include file name
    pub const TONIC_INCLUDE_FILE: &str = "buffrs.rs";

    /// Run the generator for a dependency and output files at the provided path
    pub async fn run(&self) -> Result<(), RunError> {
        let protoc = protoc_bin_path().map_err(|_| RunError::Locate)?;

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
                    .compile(&protos, includes)
                    .map_err(RunError::Io)?;
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

                let output = protoc_cmd.output().await.map_err(RunError::Io)?;

                let exit = output.status.code().ok_or(RunError::Interrupted)?;

                if exit != 0 {
                    return Err(RunError::NonZeroExitCode {
                        code: exit,
                        stderr: output.stderr,
                    });
                }

                tracing::info!(":: {language} code generated successfully");
            }
        }

        Ok(())
    }
}

/// Error produced when generating code
#[derive(Error, Debug)]
pub enum GenerateError {
    /// Could not load the manifest file from the filesystem
    #[error("Failed to read the manifest. {0}")]
    ManifestRead(manifest::ReadError),
    /// No proto files to compile
    #[error("Either a compilable package (library or api) or at least one dependency is needed to generate code bindings.")]
    NothingToGenerate,
    /// Error when executing an external code generator
    #[error("Failed to generate bindings. {0}")]
    RunFailed(RunError),
}

impl Generator {
    /// Execute code generation with pre-configured parameters
    pub async fn generate(&self) -> Result<(), GenerateError> {
        let manifest = Manifest::read()
            .await
            .map_err(GenerateError::ManifestRead)?;

        tracing::info!(":: initializing code generator");

        if !manifest.package.kind.compilable() && manifest.dependencies.is_empty() {
            return Err(GenerateError::NothingToGenerate);
        }

        self.run().await.map_err(GenerateError::RunFailed)?;

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
