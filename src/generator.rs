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

use std::{fmt, path::PathBuf};

use miette::{ensure, IntoDiagnostic};
use serde::{Deserialize, Serialize};
use tracing::{debug, info};

use crate::{manifest::Manifest, package::PackageStore};

/// The language used for code generation
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, clap::ValueEnum,
)]
#[serde(rename_all = "kebab-case")]
#[allow(missing_docs)] // trivial enum
pub enum Language {
    Cpp,
    CSharp,
    Java,
    Kotlin,
    ObjectiveC,
    Php,
    Python,
    Ruby,
    Rust,
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

impl Generator {
    /// Tonic include file name
    pub const TONIC_INCLUDE_FILE: &'static str = "buffrs.rs";

    /// Execute code generation with pre-configured parameters
    pub async fn generate(&self) -> miette::Result<()> {
        let manifest = Manifest::read().await?;
        let store = PackageStore::current().await?;

        let mut protos = vec![];

        if let Some(pkg) = &manifest.package {
            store.populate(pkg).await?;

            protos.extend(store.populated_files(pkg).await);
        } else {
            protos.extend(store.collect(&store.proto_vendor_path(), true).await);
        }

        info!(":: initializing code generator");

        ensure!(
            manifest.package.is_some() || !manifest.dependencies.is_empty() || !protos.is_empty(),
            "either a compilable package (library or api) or at least one dependency/proto file is needed to generate code bindings."
        );

        match self {
            Generator::Tonic => {
                tonic_build::configure()
                    .build_client(true)
                    .build_server(true)
                    .build_transport(true)
                    .include_file(Self::TONIC_INCLUDE_FILE)
                    .compile(&protos, &[store.proto_vendor_path()])
                    .into_diagnostic()?;
            }
            Generator::Protoc { language, out_dir } => {
                let mut protoc = protoc::ProtocLangOut::new();

                protoc.lang(match language {
                    Language::Python => "python",
                    Language::Cpp => "cpp",
                    Language::CSharp => "csharp",
                    Language::Java => "java",
                    Language::Kotlin => "kotlin",
                    Language::ObjectiveC => "objc",
                    Language::Php => "php",
                    Language::Ruby => "ruby",
                    Language::Rust => "rust",
                });

                protoc.out_dir(out_dir);

                // Setting proto path causes protoc to replace occurrences of this string appearing in the
                // path of the generated path with that provided by output path
                // e.g. if input proto path is proto/vendor/units/units.proto and the proto path is 'proto'
                // and the --python_out is 'proto/build/gen' then the file will be output to
                // proto/build/gen/vendor/units/units.py
                // We need both of these if we want "vendor" to be removed, and it has to come first
                protoc
                    .include(store.proto_vendor_path())
                    .include(store.proto_path());

                protoc.inputs(&protos);

                debug!(":: running protoc");

                protoc.run().into_diagnostic()?;

                // Python: We run again to generate type stubs
                if *language == Language::Python {
                    debug!(":: running protoc pyi");
                    protoc.lang("pyi").run().into_diagnostic()?;
                }

                info!(":: {language} code generated successfully");
            }
        }

        info!(
            ":: compiled {}[{}]",
            manifest
                .package
                .as_ref()
                .map(|p| format!("{} ", p.name))
                .unwrap_or_else(|| "".to_string()),
            store.proto_path().display()
        );

        for dependency in manifest.dependencies {
            info!(
                ":: compiled {} [{}]",
                dependency.package,
                store.locate(&dependency.package).display()
            );
        }

        Ok(())
    }
}
