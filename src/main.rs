// (c) Copyright 2023 Helsing GmbH. All rights reserved.

use std::path::{Path, PathBuf};

use buffrs::package::PackageId;
use buffrs::registry::RegistryUri;
use buffrs::{credentials::Credentials, package::PackageType};
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(author, version, about, long_about)]
#[command(propagate_version = true)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Initializes a buffrs setup
    Init {
        /// Sets up the package as lib
        #[clap(long, conflicts_with = "api")]
        #[arg(group = "pkg")]
        lib: bool,
        /// Sets up the package as api
        #[clap(long, conflicts_with = "lib")]
        #[arg(group = "pkg")]
        api: bool,
        /// The package id used for initialization
        #[clap(requires = "pkg")]
        package: Option<PackageId>,
    },

    /// Adds dependencies to a manifest file
    Add {
        /// Artifactory url (e.g. https://<domain>/artifactory)
        #[clap(long)]
        registry: RegistryUri,
        /// Dependency to add (Format <repository>/<package>@<version>
        dependency: String,
    },
    /// Removes dependencies from a manifest file
    #[clap(alias = "rm")]
    Remove {
        /// Package to remove from the dependencies
        package: PackageId,
    },

    /// Exports the current package into a distributable tgz archive
    #[clap(alias = "pack")]
    Package {
        /// Target directory for the released package
        #[clap(long)]
        #[arg(default_value = ".")]
        output_directory: String,
        /// Generate package but do not write it to filesystem
        #[clap(long)]
        dry_run: bool,
    },

    /// Packages and uploads this api to the registry
    #[clap(alias = "pub")]
    Publish {
        /// Artifactory url (e.g. https://<domain>/artifactory)
        #[clap(long)]
        registry: RegistryUri,
        /// Destination repository for the release
        #[clap(long)]
        repository: String,
        /// Allow a dirty git working tree while publishing
        #[clap(long)]
        allow_dirty: bool,
        /// Abort right before uploading the release to the registry
        #[clap(long)]
        dry_run: bool,
    },

    /// Installs dependencies
    Install,
    /// Uninstalls dependencies
    Uninstall,

    /// Generate code from installed buffrs packages
    #[clap(alias = "gen")]
    Generate {
        /// Language used for code generation
        #[clap(long = "lang")]
        #[arg(value_enum)]
        language: buffrs::generator::Language,

        #[clap(long = "out-dir")] // Defaults to None, but will use env var if set
        out_dir: PathBuf,
    },

    /// Logs you in for a registry
    Login {
        /// Artifactory url (e.g. https://<domain>/artifactory)
        #[clap(long)]
        registry: RegistryUri,
    },
    /// Logs you out from a registry
    Logout {
        /// Artifactory url (e.g. https://<domain>/artifactory)
        #[clap(long)]
        registry: RegistryUri,
    },
}

#[tokio::main]
async fn main() -> eyre::Result<()> {
    human_panic::setup_panic!();

    color_eyre::install()?;

    tracing_subscriber::fmt()
        .compact()
        .without_time()
        .with_level(false)
        .with_file(false)
        .with_target(false)
        .with_line_number(false)
        .try_init()
        .unwrap();

    let cli = Cli::parse();
    let credentials = Credentials::load().await?;

    match cli.command {
        Command::Init { lib, api, package } => {
            cmd::init(if lib {
                Some((PackageType::Lib, package))
            } else if api {
                Some((PackageType::Api, package))
            } else {
                None
            })
            .await?
        }
        Command::Add {
            registry,
            dependency,
        } => cmd::add(registry, dependency).await?,
        Command::Remove { package } => cmd::remove(package).await?,
        Command::Package {
            output_directory,
            dry_run,
        } => cmd::package(output_directory, dry_run).await?,
        Command::Publish {
            registry,
            repository,
            allow_dirty,
            dry_run,
        } => cmd::publish(credentials, registry, repository, allow_dirty, dry_run).await?,
        Command::Install {} => cmd::install(credentials).await?,
        Command::Uninstall => cmd::uninstall().await?,
        Command::Generate { language, out_dir } => cmd::generate(language, out_dir).await?,
        Command::Login { registry } => cmd::login(credentials, registry).await?,
        Command::Logout { registry } => cmd::logout(credentials, registry).await?,
    }

    Ok(())
}

mod cmd {
    /// The directory used for the generated code
    pub const BUILD_DIRECTORY: &str = "proto/build";

    use std::{
        env,
        path::{Path, PathBuf},
        sync::Arc,
    };

    use buffrs::{
        credentials::Credentials,
        generator::{self, Language},
        manifest::{Dependency, Manifest, PackageManifest},
        package::{PackageId, PackageStore, PackageType},
        registry::{Artifactory, Registry, RegistryUri},
    };
    use eyre::{ensure, Context, ContextCompat};
    use futures::future::try_join_all;
    use semver::{Version, VersionReq};

    /// Initializes the project
    pub async fn init(r#type: Option<(PackageType, Option<PackageId>)>) -> eyre::Result<()> {
        let mut manifest = Manifest::default();

        if let Some((r#type, name)) = r#type {
            const DIR_ERR: &str = "Failed to read current directory name";

            let name = match name {
                Some(name) => name,
                None => std::env::current_dir()?
                    .file_name()
                    .wrap_err(DIR_ERR)?
                    .to_str()
                    .wrap_err(DIR_ERR)?
                    .parse()?,
            };

            manifest.package = Some(PackageManifest {
                r#type,
                name,
                version: Version::new(0, 1, 0),
                description: None,
            });
        }

        ensure!(
            !Manifest::exists().await?,
            "Cant initialize existing project"
        );

        manifest.write().await?;

        PackageStore::create().await
    }

    /// Adds a dependency to this project
    pub async fn add(registry: RegistryUri, dependency: String) -> eyre::Result<()> {
        let lower_kebab = |c: char| (c.is_lowercase() && c.is_ascii_alphabetic()) || c == '-';

        let (repository, dependency) = dependency
            .trim()
            .split_once('/')
            .wrap_err("Invalid dependency specification")?;

        ensure!(
            repository.chars().all(lower_kebab),
            "Repositories must be in lower kebab case"
        );

        let repository = repository.into();

        let (package, version) = dependency
            .split_once('@')
            .wrap_err_with(|| format!("Invalid dependency specification: {dependency}"))?;

        let package = package
            .parse::<PackageId>()
            .wrap_err_with(|| format!("Invalid package id supplied: {package}"))?;

        let version = version
            .parse::<VersionReq>()
            .wrap_err_with(|| format!("Invalid version requirement supplied: {package}"))?;

        let mut manifest = Manifest::read().await?;

        manifest
            .dependencies
            .push(Dependency::new(registry, repository, package, version));

        manifest.write().await
    }

    /// Removes a dependency from this project
    pub async fn remove(package: PackageId) -> eyre::Result<()> {
        let mut manifest = Manifest::read().await?;

        let dependency = manifest
            .dependencies
            .iter()
            .find(|d| d.package == package)
            .wrap_err(eyre::eyre!(
                "Unable to remove unknown dependency {package:?}"
            ))?
            .to_owned();

        manifest.dependencies.retain(|d| *d != dependency);

        PackageStore::uninstall(&dependency.package).await.ok();

        manifest.write().await
    }

    /// Packages the api and writes it to the filesystem
    pub async fn package(directory: String, dry_run: bool) -> eyre::Result<()> {
        let package = PackageStore::release()
            .await
            .wrap_err("Failed to create release")?;

        let path = Path::new(&directory).join(format!(
            "{}-{}.tgz",
            package.manifest.name, package.manifest.version
        ));

        if !dry_run {
            std::fs::write(path, package.tgz).wrap_err("failed to write package to filesystem")?;
        }

        Ok(())
    }

    /// Publishes the api package to the registry
    pub async fn publish(
        credentials: Credentials,
        registry: RegistryUri,
        repository: String,
        allow_dirty: bool,
        dry_run: bool,
    ) -> eyre::Result<()> {
        if let Ok(repository) = git2::Repository::discover(Path::new(".")) {
            let statuses = repository
                .statuses(None)
                .wrap_err("Failed to get git status")?;

            if !allow_dirty && !statuses.is_empty() {
                tracing::error!("{} files in the working directory contain changes that were not yet committed into git:\n", statuses.len());

                statuses
                    .iter()
                    .for_each(|s| tracing::error!("{}", s.path().unwrap_or_default()));

                tracing::error!("\nTo proceed with publishing despite the uncommitted changes, pass the `--allow-dirty` flag\n");

                eyre::bail!("Unable to publish a dirty git repository");
            }
        }

        let artifactory = Artifactory::new(Arc::new(credentials), registry);

        let package = PackageStore::release()
            .await
            .wrap_err("Failed to create release")?;

        if dry_run {
            tracing::warn!(":: aborting upload due to dry run");
            return Ok(());
        }

        artifactory.publish(package, repository).await?;

        Ok(())
    }

    /// Installs dependencies
    pub async fn install(credentials: Credentials) -> eyre::Result<()> {
        let manifest = Manifest::read().await?;

        let mut install = Vec::new();

        let credentials = Arc::new(credentials);

        for dependency in manifest.dependencies {
            let artifactory =
                Artifactory::new(credentials.clone(), dependency.manifest.registry.clone());
            install.push(PackageStore::install(dependency, artifactory));
        }

        try_join_all(install)
            .await
            .wrap_err("Failed to install dependencies")?;

        Ok(())
    }

    /// Uninstalls dependencies
    pub async fn uninstall() -> eyre::Result<()> {
        PackageStore::clear().await
    }

    /// Generate bindings for a given language
    pub async fn generate(language: Language, out_dir: PathBuf) -> eyre::Result<()> {
        generator::generate(language, out_dir)
            .await
            .wrap_err_with(|| format!("Failed to generate language bindings for {language}"))?;

        Ok(())
    }

    /// Logs you in for a registry
    pub async fn login(mut credentials: Credentials, registry: RegistryUri) -> eyre::Result<()> {
        let token = {
            tracing::info!("Please enter your artifactory token:");

            let mut raw = String::new();

            std::io::stdin()
                .read_line(&mut raw)
                .wrap_err("Failed to read token")?;

            raw.trim().into()
        };

        credentials.registry_tokens.insert(registry.clone(), token);

        let artifactory = Artifactory::new(Arc::new(credentials.clone()), registry.clone());

        if env::var("BUFFRS_TESTSUITE").is_err() {
            artifactory
                .ping()
                .await
                .wrap_err("Failed to reach artifactory, please make sure the url and credentials are correct and the instance is up and running")?;
        }

        credentials.write().await
    }

    /// Logs you out from a registry
    pub async fn logout(mut credentials: Credentials, registry: RegistryUri) -> eyre::Result<()> {
        credentials.registry_tokens.remove(&registry);
        credentials.write().await
    }
}
