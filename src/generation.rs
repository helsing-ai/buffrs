use eyre::Context;

use crate::{
    manifest::{Dependency, Manifest},
    package::PackageStore,
};

/// The language used for code generation
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, clap::ValueEnum)]
pub enum Language {
    Rust,
}

/// Backend used to generate code bindings
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Generator {
    Tonic,
}

impl Generator {
    pub async fn run(&self, dependency: &Dependency) -> eyre::Result<()> {
        let protoc = protobuf_src::protoc();
        std::env::set_var("PROTOC", protoc.clone());

        match self {
            Generator::Tonic => {
                let out = format!("proto/out/{}", dependency.package.as_package_dir());
                let package = PackageStore::vendor_directory(&dependency.package);

                let protos = PackageStore::collect(&package).await;
                let includes = &[package];

                tonic_build::configure()
                    .out_dir(&out)
                    .include_file(&format!("mod.rs"))
                    .compile(&protos, includes)?;
            }
        }

        Ok(())
    }
}

/// Generate the code bindings for a language
pub async fn generate(language: Language) -> eyre::Result<()> {
    let manifest = Manifest::read().await?;

    tracing::info!(":: booting code generator for {language:#?}");

    // Only tonic is supported right now
    let generator = Generator::Tonic;

    for ref dependency in manifest.dependencies {
        generator
            .run(dependency)
            .await
            .wrap_err_with(|| format!("failed to generate bindings for {}", dependency.package))?;

        tracing::info!(
            ":: compiled {}",
            PackageStore::vendor_directory(&dependency.package).display()
        );
    }

    Ok(())
}
