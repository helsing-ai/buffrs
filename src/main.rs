// (c) Copyright 2023 Helsing GmbH. All rights reserved.

use buffrs::package::{PackageId, PackageStore};
use buffrs::{credentials::Credentials, package::PackageType};
use clap::{Parser, Subcommand};
use std::path::PathBuf;

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
        /// Dependency to add (Format <repository>/<package>@<version>
        dependency: String,
    },
    /// Removes dependencies from a manifest file
    #[clap(alias = "rm")]
    Remove {
        /// Package to remove from the dependencies
        package: PackageId,
    },

    /// Packages and uploads this api to the registry
    #[clap(alias = "pub")]
    Publish {
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
    },

    /// Logs you in for a registry
    Login {
        /// Artifactory url (e.g. https://<domain>/artifactory)
        #[clap(long)]
        url: url::Url,
        /// Artifactory username
        #[clap(long)]
        username: String,
    },
    /// Logs you out from a registry
    Logout,
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

    let config = Credentials::load().await?;

    let base_dir = PathBuf::new();
    let commands = cmd::Cmd {
        base_dir: base_dir.clone(),
        package_store: PackageStore { base_dir },
    };
    match cli.command {
        Command::Init { lib, api, package } => {
            commands
                .init(if lib {
                    Some((PackageType::Lib, package))
                } else if api {
                    Some((PackageType::Api, package))
                } else {
                    None
                })
                .await?
        }
        Command::Add { dependency } => commands.add(dependency).await?,
        Command::Remove { package } => commands.remove(package).await?,
        Command::Publish {
            repository,
            allow_dirty,
            dry_run,
        } => {
            commands
                .publish(config, repository, allow_dirty, dry_run)
                .await?
        }
        Command::Install => commands.install(config).await?,
        Command::Uninstall => commands.uninstall().await?,
        Command::Generate { language } => commands.generate(language).await?,
        Command::Login { url, username } => commands.login(config, url, username).await?,
        Command::Logout => commands.logout(config).await?,
    }

    Ok(())
}

mod cmd {
    use std::path::{Path, PathBuf};

    use buffrs::{
        credentials::Credentials,
        generator::{self, Language},
        manifest::{Dependency, Manifest, PackageManifest},
        package::{PackageId, PackageStore, PackageType},
        registry::{Artifactory, ArtifactoryConfig, Registry},
    };
    use eyre::{ensure, Context, ContextCompat};
    use futures::future::try_join_all;
    use semver::{Version, VersionReq};

    pub struct Cmd {
        pub(crate) base_dir: PathBuf,
        pub(crate) package_store: PackageStore,
    }

    impl Cmd {
        /// Initializes the project
        pub async fn init(
            &self,
            r#type: Option<(PackageType, Option<PackageId>)>,
        ) -> eyre::Result<()> {
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
                    version: Version::new(0, 0, 1),
                    description: None,
                });
            }

            ensure!(
                !Manifest::exists().await?,
                "Cant initialize existing project"
            );

            manifest.write(&self.base_dir).await?;

            self.package_store.create().await
        }

        /// Adds a dependency to this project
        pub async fn add(&self, dependency: String) -> eyre::Result<()> {
            let lower_kebab = |c: char| (c.is_lowercase() && c.is_ascii_alphabetic()) || c == '-';

            let (repository, dependency) = dependency
                .trim()
                .split_once('/')
                .wrap_err("Invalid dependency specification")?;

            ensure!(
                repository.chars().all(lower_kebab),
                "Repositories must be in lower kebab case"
            );

            let (package, version) = dependency
                .split_once('@')
                .wrap_err_with(|| format!("Invalid dependency specification: {dependency}"))?;

            let package = package
                .parse::<PackageId>()
                .wrap_err_with(|| format!("Invalid package id supplied: {package}"))?;

            let version = version
                .parse::<VersionReq>()
                .wrap_err_with(|| format!("Invalid version requirement supplied: {package}"))?;

            let mut manifest = Manifest::read(&self.base_dir).await?;

            manifest
                .dependencies
                .push(Dependency::new(repository.to_owned(), package, version));

            manifest.write(&self.base_dir).await
        }

        /// Removes a dependency from this project
        pub async fn remove(&self, package: PackageId) -> eyre::Result<()> {
            let mut manifest = Manifest::read(&self.base_dir).await?;

            let dependency = manifest
                .dependencies
                .iter()
                .find(|d| d.package != package)
                .wrap_err(eyre::eyre!(
                    "Unable to remove unknown dependency {package:?}"
                ))?
                .to_owned();

            manifest.dependencies.retain(|d| *d != dependency);

            self.package_store
                .uninstall(&dependency.package)
                .await
                .wrap_err("Failed to uninstall dependency")?;

            manifest.write(&self.base_dir).await
        }

        /// Publishes the api package to the registry
        pub async fn publish(
            &self,
            credentials: Credentials,
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

            let artifactory = {
                let Some(artifactory) = credentials.artifactory else {
                    eyre::bail!(
                    "Unable to publish package to artifactory, please login using `buffrs login`"
                );
                };

                Artifactory::from(artifactory)
            };

            let package = self
                .package_store
                .release()
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
        pub async fn install(&self, credentials: Credentials) -> eyre::Result<()> {
            let artifactory = {
                let Some(artifactory) = credentials.artifactory else {
                    eyre::bail!(
                    "Unable to install artifactory dependencies, please login using `buffrs login`"
                );
                };

                Artifactory::from(artifactory)
            };

            let manifest = Manifest::read(&self.base_dir).await?;

            let mut install = Vec::new();

            for dependency in manifest.dependencies {
                install.push(self.package_store.install(dependency, artifactory.clone()));
            }

            try_join_all(install)
                .await
                .wrap_err("Failed to install dependencies")?;

            Ok(())
        }

        /// Uninstalls dependencies
        pub async fn uninstall(&self) -> eyre::Result<()> {
            self.package_store.clear().await
        }

        /// Generate bindings for a given language
        pub async fn generate(&self, language: Language) -> eyre::Result<()> {
            generator::generate(language, &self.base_dir)
                .await
                .wrap_err_with(|| format!("Failed to generate language bindings for {language}"))?;

            Ok(())
        }

        /// Logs you in for a registry
        pub async fn login(
            &self,
            mut credentials: Credentials,
            url: url::Url,
            username: String,
        ) -> eyre::Result<()> {
            let password = {
                tracing::info!("Please enter your artifactory token:");

                let mut raw = String::new();

                std::io::stdin()
                    .read_line(&mut raw)
                    .wrap_err("Failed to read token")?;

                raw = raw.trim().to_owned();

                raw
            };

            let cfg = ArtifactoryConfig::new(url, username, password);

            let artifactory = Artifactory::from(cfg.clone());

            artifactory
                .ping()
                .await
                .wrap_err("Failed to reach artifactory, please make sure the url and credentials are correct and the instance is up and running")?;

            credentials.artifactory = Some(cfg);
            credentials.write().await
        }

        /// Logs you out from a registry
        pub async fn logout(&self, mut credentials: Credentials) -> eyre::Result<()> {
            credentials.artifactory = None;
            credentials.write().await
        }
    }
}
