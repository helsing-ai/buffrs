// (c) Copyright 2023 Helsing GmbH. All rights reserved.

use buffrs::command;
use buffrs::package::PackageName;
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
        /// The package name used for initialization
        #[clap(requires = "pkg")]
        package: Option<PackageName>,
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
        package: PackageName,
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

    match cli.command {
        Command::Init { lib, api, package } => {
            let r#type = if lib {
                PackageType::Lib
            } else if api {
                PackageType::Api
            } else {
                PackageType::Impl
            };

            command::init(r#type, package).await
        }
        Command::Add { dependency } => command::add(dependency).await,
        Command::Remove { package } => command::remove(package).await,
        Command::Package {
            output_directory,
            dry_run,
        } => command::package(output_directory, dry_run).await,
        Command::Publish {
            repository,
            allow_dirty,
            dry_run,
        } => command::publish(config, repository, allow_dirty, dry_run).await,
        Command::Install => command::install(config).await,
        Command::Uninstall => command::uninstall().await,
        Command::Generate { language } => command::generate(language).await,
        Command::Login { url } => command::login(config, url).await,
        Command::Logout => command::logout(config).await,
    }
}
