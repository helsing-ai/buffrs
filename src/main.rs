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

use std::path::PathBuf;

use buffrs::command;
use buffrs::context::Context;
use buffrs::manifest::Manifest;
use buffrs::package::PackageName;
use buffrs::registry::RegistryUri;
use buffrs::{credentials::Credentials, manifest::MANIFEST_FILE, package::PackageType};
use clap::{Parser, Subcommand};
use miette::{miette, Context as _};

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
        /// Directory where generated code should be created
        #[clap(long = "out-dir")]
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

#[tokio::main(flavor = "current_thread")]
async fn main() -> miette::Result<()> {
    human_panic::setup_panic!();

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

    // commands without a context
    match cli.command {
        Command::Init { lib, api, package } => {
            let kind = if lib {
                PackageType::Lib
            } else if api {
                PackageType::Api
            } else {
                PackageType::Impl
            };

            Context::init(kind, package.to_owned())
                .await
                .wrap_err(miette!(
                    "failed to initialize {}{kind} package",
                    package.map(|p| format!("`{p}` as ")).unwrap_or_default()
                ))?;

            return Ok(());
        }
        Command::Login { registry } => {
            let credentials = Credentials::load().await?;

            return command::login(credentials, registry.to_owned())
                .await
                .wrap_err(miette!("failed to login to `{registry}`"));
        }
        Command::Logout { registry } => {
            let credentials = Credentials::load().await?;

            return command::logout(credentials, registry.to_owned())
                .await
                .wrap_err(miette!("failed to logout from `{registry}`"));
        }
        _ => {}
    };

    let context = Context::open_current().await?;

    // handle all other commands
    match cli.command {
        Command::Init { .. } => unreachable!(),
        Command::Login { .. } => unreachable!(),
        Command::Logout { .. } => unreachable!(),
        Command::Add {
            registry,
            dependency,
        } => context
            .add(registry.to_owned(), &dependency)
            .await
            .wrap_err(miette!(
                "failed to add `{dependency}` from `{registry}` to `{MANIFEST_FILE}`"
            )),
        Command::Remove { package } => context.remove(package.to_owned()).await.wrap_err(miette!(
            "failed to remove `{package}` from `{MANIFEST_FILE}`"
        )),
        Command::Package {
            output_directory,
            dry_run,
        } => context
            .package(output_directory, dry_run)
            .await
            .wrap_err(miette!(
                "failed to export `{}` into the buffrs package format",
                Manifest::read().await?.package.name
            )),
        Command::Publish {
            registry,
            repository,
            allow_dirty,
            dry_run,
        } => context
            .publish(registry, repository, allow_dirty, dry_run)
            .await
            .wrap_err(miette!(
                "failed to publish `{}` to `{}:{}`",
                context.manifest.package.name,
                registry,
                repository
            )),
        Command::Install => context.install().await.wrap_err(miette!(
            "failed to install dependencies for `{}`",
            Manifest::read().await?.package.name,
        )),
        Command::Uninstall => context.uninstall().await.wrap_err(miette!(
            "failed to install dependencies for `{}`",
            Manifest::read().await?.package.name,
        )),
        Command::Generate { language, out_dir } => context
            .generate(language, out_dir)
            .await
            .wrap_err(miette!("failed to generate {language} language bindings")),
    }
}
