use std::{
    fmt,
    path::{Path, PathBuf},
};

use eyre::Context;
use protoc_bin_vendored::protoc_bin_path;
use serde::{Deserialize, Serialize};
use tokio::fs;

use crate::{
    manifest::Manifest,
    package::{PackageId, PackageStore},
};

/// The directory used for the generated code
pub const BUILD_DIRECTORY: &str = "proto/build";

/// The language used for code generation
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, clap::ValueEnum,
)]
#[serde(rename_all = "kebab-case")]
pub enum Language {
    Rust,
}

impl Language {
    pub fn build_directory(&self) -> PathBuf {
        Path::new(BUILD_DIRECTORY).join(self.to_string())
    }
}

impl fmt::Display for Language {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", serde_typename::to_str(&self).unwrap_or("unknown"))
    }
}

/// A configuration passed to a generator to compile a package
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct GenerationConfig<'i, 'p> {
    pub package: &'i PackageId,
    pub location: &'p Path,
    pub out: &'p Path,
}

/// Backend used to generate code bindings
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Generator {
    /// The tonic + prost stack
    Tonic,
}

impl Generator {
    pub const TONIC_INCLUDE_FILE: &str = "mod.rs";

    /// Run the generator for a dependency and output files into `out`
    pub async fn run(&self, config: GenerationConfig<'_, '_>) -> eyre::Result<()> {
        let protoc = protoc_bin_path().wrap_err("Unable to locate vendored protoc")?;

        std::env::set_var("PROTOC", protoc.clone());

        let out = config.out.join(config.package.as_str());

        fs::remove_dir_all(&out).await.ok();

        fs::create_dir_all(&out)
            .await
            .wrap_err("Failed to recreate dependency output directory")?;

        match self {
            Generator::Tonic => {
                let protos = PackageStore::collect(config.location).await;

                let includes = &[&config.location];

                tonic_build::configure()
                    .build_client(true)
                    .build_server(true)
                    .build_transport(true)
                    .compile_well_known_types(true)
                    .out_dir(&out)
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
        manifest.package.is_some() || !manifest.dependencies.is_empty(),
        "Either a local package or at least one dependency is needed to generate code bindings."
    );

    // Only tonic is supported right now
    let generator = Generator::Tonic;

    let out = {
        let out = language.build_directory();

        fs::remove_dir_all(&out).await.ok();

        fs::create_dir_all(&out).await.wrap_err(eyre::eyre!(
            "Failed to create clean build directory {} for {language}",
            out.canonicalize()?.to_string_lossy()
        ))?;

        out
    };

    if let Some(ref pkg) = manifest.package {
        let location = Path::new(PackageStore::PROTO_PATH);

        generator
            .run(GenerationConfig {
                package: &pkg.name,
                location,
                out: &out,
            })
            .await
            .wrap_err_with(|| format!("Failed to generate bindings for {}", pkg.name))?;

        tracing::info!(":: compiled {} [{}]", pkg.name, location.display());
    }

    for dependency in manifest.dependencies {
        let location = PackageStore::locate(&dependency.package);

        generator
            .run(GenerationConfig {
                package: &dependency.package,
                location: &location,
                out: &out,
            })
            .await
            .wrap_err_with(|| format!("Failed to generate bindings for {}", dependency.package))?;

        tracing::info!(
            ":: compiled {} [{}]",
            dependency.package,
            location.display()
        );
    }

    Ok(())
}

/// Include the rust language bindings of a buffrs dependency
///
/// You must specify the buffrs dependency package id.
///
/// ```rust,ignore
/// mod protos {
///     buffrs::include!("demo");
/// }
/// ```
#[macro_export]
macro_rules! include {
    ($package:expr) => {
        ::std::include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/proto/build/rust/",
            $package,
            "/mod.rs"
        ));
    };
}
