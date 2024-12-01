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

use std::env;

use buffrs::command::{self, GenerationOption, InstallMode};
use buffrs::config::Config;
use buffrs::manifest::Manifest;
use buffrs::package::PackageName;
use buffrs::registry::CertValidationPolicy;
use buffrs::{manifest::MANIFEST_FILE, package::PackageType};
use clap::{CommandFactory, Parser, Subcommand};
use miette::{miette, IntoDiagnostic, WrapErr};
use semver::Version;

#[derive(Parser)]
#[command(author, version, about, long_about)]
#[command(propagate_version = true)]
struct Cli {
    /// Opt out of applying default arguments from config
    #[clap(long)]
    ignore_defaults: bool,

    /// Disable certificate validation
    ///
    /// By default, every secure connection buffrs makes will validate the certificate chain.
    /// This option makes buffrs skip the verification step and proceed without checking.
    #[clap(long, long = "insecure", short = 'k')]
    disable_cert_validation: bool,

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
        generate_buf_yaml: bool,
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

        /// Token to use for login (if not provided, will prompt for input)
        #[clap(long)]
        token: Option<String>,
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

    // The CLI handling is part of the library crate.
    // This allows build.rs scripts to simply declare buffrs as
    // a dependency and use the CLI without any additional setup.
    let args = env::args().collect::<Vec<_>>();
    run(&args).await
}

/// Main entry point for the CLI
///
/// This function is part of the library crate to allow for `buffrs` to be
/// called from a `build.rs` script.
async fn run(args: &[String]) -> miette::Result<()> {
    let cwd = std::env::current_dir().into_diagnostic()?;
    let config = Config::new(Some(&cwd))?;

    // Merge default arguments with user-specified arguments
    let merged_args = merge_args_with_defaults(&config, args);

    // Parse CLI with merged arguments
    let cli = Cli::parse_from(merged_args);

    let manifest = if Manifest::exists().await? {
        Some(Manifest::read(&config).await?)
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

    let policy = if cli.disable_cert_validation {
        CertValidationPolicy::NoValidation
    } else {
        CertValidationPolicy::Validate
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
        Command::Login { registry, token } => {
            let registry = config.parse_registry_arg(&registry)?;
            command::login(&registry, token, policy, &config)
                .await
                .wrap_err(miette!("failed to login to `{registry}`"))
        }
        Command::Logout { registry } => {
            let registry = config.parse_registry_arg(&registry)?;
            command::logout(&registry, &config)
                .await
                .wrap_err(miette!("failed to logout from `{registry}`"))
        }
        Command::Add {
            registry,
            dependency,
        } => {
            let registry = config.parse_registry_arg(&registry)?;
            command::add(&registry, &dependency, &config, policy)
                .await
                .wrap_err(miette!(
                    "failed to add `{dependency}` from `{registry}` to `{MANIFEST_FILE}`"
                ))
        }
        Command::Remove { package } => {
            command::remove(package.to_owned(), &config)
                .await
                .wrap_err(miette!(
                    "failed to remove `{package}` from `{MANIFEST_FILE}`"
                ))
        }
        Command::Package {
            output_directory,
            dry_run,
            set_version,
        } => command::package(output_directory, dry_run, set_version, &config)
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
            let registry = config.parse_registry_arg(&registry)?;
            command::publish(
                &registry,
                repository.to_owned(),
                allow_dirty,
                dry_run,
                set_version,
                &config,
                policy,
            )
            .await
            .wrap_err(miette!(
                "failed to publish `{package}` to `{registry}:{repository}`",
            ))
        }
        Command::Lint => command::lint(&config)
            .await
            .wrap_err(miette!("failed to lint protocol buffers",)),
        Command::Install {
            only_dependencies,
            generate_buf_yaml,
        } => {
            let mut generation_options = Vec::new();

            if generate_buf_yaml {
                generation_options.push(GenerationOption::BufYaml);
            }

            let install_mode = if only_dependencies {
                InstallMode::DependenciesOnly
            } else {
                InstallMode::All
            };

            command::install(install_mode, &generation_options, &config, policy)
                .await
                .wrap_err(miette!("failed to install dependencies for `{package}`"))
        }
        Command::Uninstall => command::uninstall()
            .await
            .wrap_err(miette!("failed to uninstall dependencies for `{package}`")),
        Command::List => command::list(&config).await.wrap_err(miette!(
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
/// * `args` - The user-provided arguments
///
/// # Returns
/// A vector of arguments with default arguments merged in
pub fn merge_args_with_defaults(config: &Config, args: &[String]) -> Vec<String> {
    // Check if --ignore-defaults is in the arguments
    let initial_cli = Cli::try_parse_from(args);
    if let Ok(cli) = initial_cli {
        if cli.ignore_defaults {
            return args.to_vec(); // Return original arguments if --ignore-defaults is set
        }
    }

    // Parse the CLI matches to find the subcommand
    let cli_matches = Cli::command().get_matches_from(args);
    let mut args = args.to_vec();

    // Fetch sub-command-specific defaults if a subcommand is present
    if let Some(subcommand) = cli_matches.subcommand_name() {
        let command_specific_args = config.get_default_args(Some(subcommand));

        // Find the position of the subcommand in the arguments
        if let Some(position) = args.iter().position(|arg| arg == subcommand) {
            // Insert command-specific defaults after the subcommand
            let user_args: std::collections::HashSet<_> = args.iter().collect();
            let filtered_defaults: Vec<String> = command_specific_args
                .into_iter()
                .filter(|arg| !user_args.contains(arg))
                .collect();
            args.splice(position + 1..position + 1, filtered_defaults);
        }
    }

    // Always fetch common defaults
    let mut default_args = config.get_default_args(None);

    // Prepend common defaults before all other arguments, filtering duplicates
    let user_args: std::collections::HashSet<_> = args.iter().collect();
    default_args.retain(|arg| !user_args.contains(arg));
    args.splice(1..1, default_args);

    args
}
