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

use buffrs::command;
use buffrs::manifest::Manifest;
use buffrs::package::{PackageName, PackageStore};
use buffrs::registry::RegistryUri;
use buffrs::{manifest::MANIFEST_FILE, package::PackageType};
use clap::{Parser, Subcommand};
use miette::{miette, WrapErr};
use semver::Version;

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

    /// Check rule violations for this package.
    Lint,

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
        /// Override the version from the manifest
        ///
        /// Note: This overrides the version in the manifest.
        #[clap(long)]
        set_version: Option<Version>,
    },

    /// Packages and uploads this api to the registry
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
        /// Override the version from the manifest
        ///
        /// Note: This overrides the version in the manifest.
        #[clap(long)]
        set_version: Option<Version>,
    },

    /// Installs dependencies
    Install,
    /// Uninstalls dependencies
    Uninstall,

    /// Lists all protobuf files managed by Buffrs to stdout
    #[clap(alias = "ls")]
    List,

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

    /// Lockfile related commands
    Lock {
        #[command(subcommand)]
        command: LockfileCommand,
    },
}

#[derive(Subcommand)]
enum LockfileCommand {
    /// Prints the file requirements derived from the lockfile serialized as JSON
    ///
    /// This is useful for consumption of the lockfile in other programs.
    PrintFiles,
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

    let manifest = if Manifest::exists().await? {
        Some(Manifest::read().await?)
    } else {
        None
    };

    let package = {
        let cwd = std::env::current_dir().unwrap();

        let name = cwd
            .file_name()
            .ok_or_else(|| miette!("failed to locate current directory"))?
            .to_str()
            .ok_or_else(|| miette!("internal error"))?;

        manifest
            .and_then(|m| m.package.map(|p| p.name.to_string()))
            .unwrap_or_else(|| name.to_string())
    };

    match cli.command {
        Command::Init { lib, api, package } => {
            let kind = if lib {
                Some(PackageType::Lib)
            } else if api {
                Some(PackageType::Api)
            } else {
                None
            };

            command::init(kind, package.to_owned())
                .await
                .wrap_err(miette!(
                    "failed to initialize {}",
                    package.map(|p| format!("`{p}`")).unwrap_or_default()
                ))
        }
        Command::Login { registry } => command::login(registry.to_owned())
            .await
            .wrap_err(miette!("failed to login to `{registry}`")),
        Command::Logout { registry } => command::logout(registry.to_owned())
            .await
            .wrap_err(miette!("failed to logout from `{registry}`")),
        Command::Add {
            registry,
            dependency,
        } => command::add(registry.to_owned(), &dependency)
            .await
            .wrap_err(miette!(
                "failed to add `{dependency}` from `{registry}` to `{MANIFEST_FILE}`"
            )),
        Command::Remove { package } => command::remove(package.to_owned()).await.wrap_err(miette!(
            "failed to remove `{package}` from `{MANIFEST_FILE}`"
        )),
        Command::Package {
            output_directory,
            dry_run,
            set_version,
        } => command::package(output_directory, dry_run, set_version)
            .await
            .wrap_err(miette!(
                "failed to export `{package}` into the buffrs package format"
            )),
        Command::Publish {
            registry,
            repository,
            allow_dirty,
            dry_run,
            set_version,
        } => command::publish(
            registry.to_owned(),
            repository.to_owned(),
            allow_dirty,
            dry_run,
            set_version,
        )
        .await
        .wrap_err(miette!(
            "failed to publish `{package}` to `{registry}:{repository}`",
        )),
        Command::Lint => command::lint().await.wrap_err(miette!(
            "failed to lint protocol buffers in `{}`",
            PackageStore::PROTO_PATH
        )),
        Command::Install => command::install()
            .await
            .wrap_err(miette!("failed to install dependencies for `{package}`")),
        Command::Uninstall => command::uninstall()
            .await
            .wrap_err(miette!("failed to uninstall dependencies for `{package}`")),
        Command::List => command::list().await.wrap_err(miette!(
            "failed to list installed protobuf files for `{package}`"
        )),
        Command::Lock { command } => match command {
            LockfileCommand::PrintFiles => command::lock::print_files().await.wrap_err(miette!(
                "failed to print locked file requirements of `{package}`"
            )),
        },
    }
}
