// Copyright 2024 Globus Medical, Inc.
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
use miette::{bail, ensure, miette, Context, IntoDiagnostic};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    str::FromStr,
};

use crate::{
    manifest::{Edition, CANARY_EDITION},
    registry::{sanity_check_url, RegistryUri},
};

/// Location of the configuration file
const CONFIG_FILE: &str = ".buffrs/config.toml";

/// Key for common default arguments
const DEFAULT_ARGS_KEY: &str = "*";

/// Representation of the configuration file
///
/// # Example
///
/// ```toml
/// edition = "0.10.0"
///
/// [registries]
/// some_org = "https://artifactory.example.com/artifactory/some-org"
///
/// [registry]
/// default = "some_org"
///
/// [commands]
/// default_args = ["--insecure"]
///
/// [commands.install]
/// default_args = ["--buf-yaml"]
/// ```
///
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// Edition of this configuration file (in sync with the Proto.toml edition)
    edition: Edition,

    /// Path to the configuration file
    config_path: Option<PathBuf>,

    /// Default registry alias to use if none is specified
    default_registry: Option<String>,

    /// List of registries
    registries: HashMap<String, url::Url>,

    /// Default arguments for commands
    command_defaults: HashMap<String, Vec<String>>,
}

impl Config {
    /// Create a new configuration with default values
    /// # Arguments
    /// * `cwd` - Starting directory to search for the configuration file
    ///
    pub fn new(cwd: Option<&Path>) -> miette::Result<Self> {
        match Self::locate_config(cwd) {
            Some(config_path) => Self::new_from_config_file(&config_path),
            None => Ok(Self {
                edition: Edition::latest(),
                config_path: None,
                default_registry: None,
                registries: HashMap::new(),
                command_defaults: HashMap::new(),
            }),
        }
    }

    /// Parse a registry argument
    ///
    /// # Arguments
    /// * `registry` - The registry argument to parse
    ///
    /// # Returns
    /// URI with either alias scheme or actual URI:
    /// - <alias> -> alias://<alias>
    /// - <uri> -> <uri>
    /// - None -> alias://<default>
    pub fn parse_registry_arg(&self, registry: &Option<String>) -> miette::Result<RegistryUri> {
        match registry {
            Some(registry) => RegistryUri::from_str(registry),
            None => match &self.default_registry {
                Some(default_registry) => RegistryUri::from_str(default_registry),
                None => bail!("no registry provided and no default registry found"),
            },
        }
    }

    /// Lookup a registry by name
    ///
    /// # Arguments
    /// * `name` - Name of the registry to lookup
    ///
    /// # Returns
    /// The registry URI
    pub fn lookup_registry(&self, name: &str) -> miette::Result<url::Url> {
        self.registries.get(name).cloned().ok_or_else(|| {
            miette!(
                "registry '{}' not found in {}",
                name,
                self.config_path
                    .clone()
                    .unwrap_or("config file".into())
                    .display()
            )
        })
    }

    /// Get the default arguments for a specific command
    ///
    /// # Arguments
    /// * `command` - The command name to get default arguments for, or None for global defaults
    ///
    /// # Returns
    /// A vector of default arguments for the specified command
    pub fn get_default_args(&self, command: Option<&str>) -> Vec<String> {
        self.command_defaults
            .get(command.unwrap_or(DEFAULT_ARGS_KEY))
            .cloned()
            .unwrap_or_default()
    }

    /// Locate the configuration file in the current directory or any parent directories
    ///
    /// # Arguments
    /// * `cwd` - Starting directory to search for the configuration file
    ///
    /// # Returns
    /// Some(PathBuf) if the configuration file is found, None otherwise
    pub fn locate_config(cwd: Option<&Path>) -> Option<PathBuf> {
        if let Some(cwd) = cwd {
            let mut current_dir = cwd.to_owned();

            loop {
                let config_path = current_dir.join(CONFIG_FILE);
                if config_path.exists() {
                    return Some(config_path);
                }

                if !current_dir.pop() {
                    break;
                }
            }
        }

        None
    }

    /// Create configuration from a TOML file
    ///
    /// # Arguments
    /// * `config_path` - Path to the configuration file
    fn new_from_config_file(config_path: &Path) -> miette::Result<Self> {
        let config = Self::parse_config(config_path)?;

        // Load edition from root of the config file
        let edition = config
            .get("edition")
            .and_then(|edition| edition.as_str())
            .ok_or_else(|| miette!("missing or invalid 'edition' field in config file"))?
            .into();

        match edition {
            Edition::Canary => (),
            _ => bail!("unsupported config file edition, supported editions: {CANARY_EDITION}"),
        }

        // Load registries from [registries] section
        let registries = Self::get_registries(&config, config_path)?;

        // Locate default registry from [registry.default]
        let default_registry = Self::get_default_registry(&config, &registries)?;

        // Parse command-specific default arguments from [commands.*] sections
        let command_defaults = Self::get_command_defaults(config, config_path)?;

        Ok(Self {
            edition,
            config_path: Some(config_path.to_owned()),
            default_registry,
            registries,
            command_defaults,
        })
    }

    fn parse_config(config_path: &Path) -> Result<toml::Value, miette::Error> {
        let config_str = std::fs::read_to_string(config_path)
            .into_diagnostic()
            .wrap_err(miette!(
                "failed to read config file: {}",
                config_path.display()
            ))?;
        let config: toml::Value =
            toml::from_str(&config_str)
                .into_diagnostic()
                .wrap_err(miette!(
                    "failed to parse config file: {}",
                    config_path.display()
                ))?;
        Ok(config)
    }

    fn get_default_registry(
        config: &toml::Value,
        registries: &HashMap<String, url::Url>,
    ) -> miette::Result<Option<String>> {
        let default_registry = config
            .get("registry")
            .and_then(|registry| registry.get("default"))
            .and_then(|default| default.as_str())
            .map(|default| default.to_string());
        if let Some(ref default_registry) = default_registry {
            ensure!(
                registries.contains_key(default_registry),
                "default registry '{}' not found in list of registries",
                default_registry
            );
        }
        Ok(default_registry)
    }

    fn get_registries(
        config: &toml::Value,
        config_path: &Path,
    ) -> miette::Result<HashMap<String, url::Url>> {
        let registries = config
            .get("registries")
            .and_then(|registries| registries.as_table())
            .map(|registries| {
                registries
                    .iter()
                    .map(|(name, uri)| {
                        let uri = uri
                            .as_str()
                            .ok_or_else(|| miette!("registry URI must be a string"))
                            .wrap_err(miette!("invalid URI for registry '{}'", name))
                            .wrap_err(miette!("in config file: {}", config_path.display()))?;
                        let uri = url::Url::from_str(uri).into_diagnostic()?;
                        sanity_check_url(&uri)?;
                        Ok((name.to_owned(), uri))
                    })
                    .collect::<miette::Result<HashMap<String, url::Url>>>()
            })
            .unwrap_or_else(|| Ok(HashMap::new()))
            .wrap_err(miette!(
                "failed to load registries from config file: {}",
                config_path.display()
            ))?;
        Ok(registries)
    }

    fn get_command_defaults(
        config: toml::Value,
        config_path: &Path,
    ) -> miette::Result<HashMap<String, Vec<String>>> {
        let mut command_defaults = config
            .get("commands")
            .and_then(|commands| commands.as_table())
            .map(|commands| {
                commands
                    .iter()
                    .map(|(command, settings)| {
                        let default_args = settings
                            .get("default_args")
                            .and_then(|args| args.as_array())
                            .map(|args| {
                                args.iter()
                                    .filter_map(|arg| arg.as_str().map(|s| s.to_string()))
                                    .collect::<Vec<String>>()
                            })
                            .unwrap_or_default();
                        Ok((command.to_string(), default_args))
                    })
                    .collect::<miette::Result<HashMap<String, Vec<String>>>>()
            })
            .unwrap_or_else(|| Ok(HashMap::new()))
            .wrap_err(miette!(
                "failed to load command defaults from config file: {}",
                config_path.display()
            ))?;

        // Load common default arguments
        if let Some(global_args) = config
            .get("commands")
            .and_then(|commands| commands.get("default_args"))
            .and_then(|args| args.as_array())
        {
            let global_defaults = global_args
                .iter()
                .filter_map(|arg| arg.as_str().map(|s| s.to_string()))
                .collect::<Vec<String>>();
            command_defaults.insert(DEFAULT_ARGS_KEY.to_string(), global_defaults);
        }

        Ok(command_defaults)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use assert_fs::TempDir;
    use std::{fs::File, io::Write};

    #[test]
    fn test_new_from_config_file() {
        let tmp_dir = TempDir::new().unwrap();
        let config_path = tmp_dir.path().join(CONFIG_FILE);
        std::fs::create_dir_all(config_path.parent().unwrap()).unwrap();
        let mut file = File::create(&config_path).unwrap();
        file.write_all(
            br#"
edition = "0.10"

[registry]
default = "acme"

[registries]
acme = "https://conan.acme.com/artifactory"

[commands]
default_args = ["--insecure"]

[commands.install]
default_args = ["--buf-yaml", "--proto-rs", "src"]
"#,
        )
        .unwrap();

        let config = Config::new_from_config_file(&config_path).unwrap();
        assert_eq!(config.edition, Edition::latest());
        assert_eq!(config.default_registry, Some("acme".to_string()));
        assert_eq!(
            config.registries.get("acme").unwrap(),
            &"https://conan.acme.com/artifactory".parse().unwrap()
        );
        assert_eq!(
            config.command_defaults.get("install").unwrap(),
            &vec![
                "--buf-yaml".to_string(),
                "--proto-rs".to_string(),
                "src".to_string()
            ]
        );
        assert_eq!(
            config.command_defaults.get(DEFAULT_ARGS_KEY).unwrap(),
            &vec!["--insecure".to_string()]
        );
    }
}
