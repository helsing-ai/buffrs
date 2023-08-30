use std::{
    fmt,
    path::{Path, PathBuf},
};

use eyre::Context;
use protoc_bin_vendored::protoc_bin_path;
use serde::{Deserialize, Serialize};
use tokio::fs;

use crate::{manifest::Manifest, package::PackageStore};

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

/// Backend used to generate code bindings
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Generator {
    /// The tonic + prost stack
    Tonic,
}

impl Generator {
    pub const TONIC_INCLUDE_FILE: &str = "mod.rs";

    /// Run the generator for a dependency and output files at the provided path
    pub async fn run(
        &self,
        base_dir: impl AsRef<Path>,
        out_dir: impl AsRef<Path>,
    ) -> eyre::Result<()> {
        let protoc = protoc_bin_path().wrap_err("Unable to locate vendored protoc")?;

        std::env::set_var("PROTOC", protoc.clone());

        let package_store = PackageStore {
            base_dir: base_dir.as_ref().to_path_buf(),
        };
        let protos = package_store.collect().await?;

        match self {
            Generator::Tonic => {
                tonic_build::configure()
                    .build_client(true)
                    .build_server(true)
                    .build_transport(true)
                    .compile_well_known_types(true)
                    .out_dir(out_dir)
                    .include_file(Self::TONIC_INCLUDE_FILE)
                    .compile(&protos, &[package_store.proto_path()])?;
            }
        }

        Ok(())
    }
}

/// Generate the code bindings for a language
pub async fn generate(language: Language, base_dir: &PathBuf) -> eyre::Result<()> {
    let manifest = Manifest::read(base_dir).await?;

    tracing::info!(":: initializing code generator for {language}");

    eyre::ensure!(
        manifest.package.is_some() || !manifest.dependencies.is_empty(),
        "Either a local package or at least one dependency is needed to generate code bindings."
    );

    // Only tonic is supported right now
    let generator = Generator::Tonic;

    let out_dir = {
        let out_dir = language.build_directory();

        fs::remove_dir_all(&out_dir).await.ok();

        fs::create_dir_all(&out_dir).await.wrap_err(eyre::eyre!(
            "Failed to create clean build directory {} for {language}",
            out_dir.canonicalize()?.to_string_lossy()
        ))?;

        out_dir
    };

    generator
        .run(base_dir, &out_dir)
        .await
        .wrap_err_with(|| format!("Failed to generate bindings for {language}"))?;

    let package_store = PackageStore {
        base_dir: base_dir.clone(),
    };

    if let Some(ref pkg) = manifest.package {
        tracing::info!(":: compiled {} [{}]", pkg.name, package_store.proto_path().display());
    }

    for dependency in manifest.dependencies {
        let location = package_store.locate(&dependency.package);
        tracing::info!(
            ":: compiled {} [{}]",
            dependency.package,
            location.display()
        );
    }

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
        ::std::include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/proto/build/rust/mod.rs",
        ));
    };
}
