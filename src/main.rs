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

use buffrs::command::{self, GenerationFlags, InstallMode};
use buffrs::config::Config;
use buffrs::manifest::Manifest;
use buffrs::package::PackageName;
use buffrs::{manifest::MANIFEST_FILE, package::PackageType};
use clap::CommandFactory;
use clap::{Parser, Subcommand};
use miette::{miette, IntoDiagnostic, WrapErr};
use semver::Version;

#[derive(Parser)]
#[command(author, version, about, long_about)]
#[command(propagate_version = true)]
struct Cli {
    /// Opt out of applying default arguments from config
    #[clap(long)]
    ignore_defaults: bool,

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

    /// Creates a new buffrs package in the current directory
    New {
        /// Sets up the package as lib
        #[clap(long, conflicts_with = "api")]
        #[arg(group = "pkg")]
        lib: bool,
        /// Sets up the package as api
        #[clap(long, conflicts_with = "lib")]
        #[arg(group = "pkg")]
        api: bool,
        /// The package name
        #[clap(requires = "pkg")]
        package: PackageName,
    },

    /// Check rule violations for this package.
    Lint,

    /// Adds dependencies to a manifest file
    Add {
        /// Artifactory url (e.g. https://<domain>/artifactory)
        #[clap(long)]
        registry: Option<String>,
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
        registry: Option<String>,
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
    Install {
        /// Only install dependencies
        #[clap(long, default_value = "false")]
        only_dependencies: bool,

        /// Generate buf.yaml file matching the installed dependencies
        #[clap(long, default_value = "false")]
        buf_yaml: bool,
    },

    /// Uninstalls dependencies
    Uninstall,

    /// Lists all protobuf files managed by Buffrs to stdout
    #[clap(alias = "ls")]
    List,

    /// Logs you in for a registry
    Login {
        /// Artifactory url (e.g. https://<domain>/artifactory)
        #[clap(long)]
        registry: Option<String>,
    },
    /// Logs you out from a registry
    Logout {
        /// Artifactory url (e.g. https://<domain>/artifactory)
        #[clap(long)]
        registry: Option<String>,
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

    let cwd = std::env::current_dir().into_diagnostic()?;

    let config = Config::new(Some(&cwd))?;

    // Merge default arguments with user-specified arguments
    let args = merge_with_default_args(&config);

    // Parse CLI with merged arguments
    let cli = Cli::parse_from(args);

    let manifest = if Manifest::exists().await? {
        Some(Manifest::read().await?)
    } else {
        None
    };

    let package = {
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
            let kind = infer_package_type(lib, api);

            command::init(kind, package.to_owned())
                .await
                .wrap_err(miette!(
                    "failed to initialize {}",
                    package.map(|p| format!("`{p}`")).unwrap_or_default()
                ))
        }
        Command::New { lib, api, package } => {
            let kind = infer_package_type(lib, api);

            command::new(kind, package.to_owned())
                .await
                .wrap_err(miette!("failed to initialize {}", format!("`{package}`")))
        }
        Command::Login { registry } => {
            let registry = config.resolve_registry_string(&registry)?;
            command::login(&registry, None)
                .await
                .wrap_err(miette!("failed to login to `{registry}`"))
        }
        Command::Logout { registry } => {
            let registry = config.resolve_registry_string(&registry)?;
            command::logout(&registry)
                .await
                .wrap_err(miette!("failed to logout from `{registry}`"))
        }
        Command::Add {
            registry,
            dependency,
        } => {
            let registry = config.parse_registry_arg(&registry)?;
            let resolved_registry = config.resolve_registry_uri(&registry)?;
            command::add(&registry, &resolved_registry, &dependency)
                .await
                .wrap_err(miette!(
                    "failed to add `{dependency}` from `{registry}` to `{MANIFEST_FILE}`"
                ))
        }
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
        } => {
            let registry = config.resolve_registry_string(&registry)?;
            command::publish(
                &registry,
                repository.to_owned(),
                allow_dirty,
                dry_run,
                set_version,
            )
            .await
            .wrap_err(miette!(
                "failed to publish `{package}` to `{registry}:{repository}`",
            ))
        }
        Command::Lint => command::lint()
            .await
            .wrap_err(miette!("failed to lint protocol buffers",)),
        Command::Install {
            only_dependencies,
            buf_yaml,
        } => {
            let mut generation_flags = GenerationFlags::empty();
            if buf_yaml {
                generation_flags |= GenerationFlags::BUF_YAML;
            }

            let install_mode = if only_dependencies {
                InstallMode::DependenciesOnly
            } else {
                InstallMode::All
            };

            command::install(install_mode, generation_flags, &config)
                .await
                .wrap_err(miette!("failed to install dependencies for `{package}`"))
        }
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

fn infer_package_type(lib: bool, api: bool) -> Option<PackageType> {
    if lib {
        Some(PackageType::Lib)
    } else if api {
        Some(PackageType::Api)
    } else {
        None
    }
}

/// Retrieve and merge default arguments with user-provided arguments
///
/// # Arguments
/// * `config` - The configuration object
///
/// # Returns
/// A vector of arguments with default arguments merged in
fn merge_with_default_args(config: &Config) -> Vec<String> {
    let mut args: Vec<String> = std::env::args().collect();

    // Check if --ignore-defaults is in the arguments
    let initial_cli = Cli::try_parse_from(&args);
    if let Ok(cli) = initial_cli {
        if cli.ignore_defaults {
            return args; // Return original arguments if --ignore-defaults is set
        }
    }

    // Determine the command name based on the user's input
    let cli_matches = Cli::command().get_matches_from(args.clone());

    // Find the position of the subcommand in the arguments
    if let Some((subcommand, _)) = cli_matches.subcommand() {
        let command_position = args
            .iter()
            .position(|arg| arg == subcommand)
            .unwrap_or_else(|| args.len() - 1);

        // Get default args for this command
        let default_args = config.get_default_args(subcommand);
        if !default_args.is_empty() {
            // Insert default arguments right after the command position
            args.splice(command_position + 1..command_position + 1, default_args);
        }
    }

    args
}
