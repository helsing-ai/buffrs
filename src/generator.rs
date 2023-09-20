use std::{fmt, path::Path};

use eyre::Context;
use protoc_bin_vendored::protoc_bin_path;
use serde::{Deserialize, Serialize};

use crate::{manifest::Manifest, package::PackageStore};

/// The language used for code generation
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, clap::ValueEnum,
)]
#[serde(rename_all = "kebab-case")]
pub enum Language {
    Rust,
    Python,
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
    // Protoc,
}

impl Generator {
    pub const TONIC_INCLUDE_FILE: &str = "buffrs.rs";

    /// Run the generator for a dependency and output files at the provided path
    pub async fn run(&self, output_directory: String) -> eyre::Result<()> {
        let protoc = protoc_bin_path().wrap_err("Unable to locate vendored protoc")?;

        std::env::set_var("PROTOC", protoc.clone());

        let store = Path::new(PackageStore::PROTO_PATH);
        let protos = PackageStore::collect(store).await;
        let includes = &[store];

        match self {
            Generator::Tonic => {
                tonic_build::configure()
                    .out_dir(output_directory) // If OUT_DIR is not set, Tonic throws a rather unhelpful error
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
pub async fn generate(language: Language, output_directory: String) -> eyre::Result<()> {
    let manifest = Manifest::read().await?;

    tracing::info!(":: initializing code generator for {language}");

    eyre::ensure!(
        manifest.package.is_some() || !manifest.dependencies.is_empty(),
        "Either a local package or at least one dependency is needed to generate code bindings."
    );

    // Which directory should we use - prefer the one set in env var OUT_DIR, but allow a fall backk
    // let output_directory: String = match std::env::var("OUT_DIR") {
    //     Ok(dir) => dir,
    //     Err(_) => {
    //         tracing::warn!(":: outputting to default location: {BUILD_DIRECTORY}");
    //         BUILD_DIRECTORY.to_string()
    //     }
    // };

    // Only tonic is supported right now
    let generator = Generator::Tonic;

    generator
        .run(output_directory)
        .await
        .wrap_err_with(|| format!("Failed to generate bindings for {language}"))?;

    if let Some(ref pkg) = manifest.package {
        let location = Path::new(PackageStore::PROTO_PATH);
        tracing::info!(":: compiled {} [{}]", pkg.name, location.display());
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
