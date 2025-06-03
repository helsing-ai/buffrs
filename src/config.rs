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
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use miette::{bail, miette, Context, IntoDiagnostic};
use serde::{de, Deserialize, Deserializer, Serialize};

use crate::{
    manifest::{Edition, CANARY_EDITION},
    registry::{RegistryAlias, RegistryRef, RegistryUri},
};

/// Location of the configuration file
const CONFIG_FILE: &str = ".buffrs/config.toml";

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
/// default_args = ["--generate-buf-yaml"]
/// ```
///
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Config {
    /// Edition of this configuration file (in sync with the Proto.toml edition)
    #[serde(deserialize_with = "validate_edition")]
    edition: Edition,

    /// Path to the configuration file
    #[serde(skip)]
    config_path: Option<PathBuf>,

    /// Default registry alias to use if none is specified
    #[serde(default)]
    pub registry: RegistryConfig,

    /// List of registries
    #[serde(default)]
    registries: HashMap<RegistryAlias, RegistryUri>,

    /// Default arguments for commands
    #[serde(rename = "commands", default)]
    command_defaults: Commands,
}

/// Registry configuration
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct RegistryConfig {
    /// Default registry alias to use if none is specified
    pub default: Option<String>,
}

/// Commands configuration, including global and per-command default arguments
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Commands {
    /// Default arguments for all commands
    #[serde(rename = "default_args", default)]
    pub common: Option<Vec<String>>,

    /// Specific command configurations
    #[serde(flatten)]
    pub specific: HashMap<String, CommandConfig>,
}

/// Per-command configuration
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandConfig {
    /// Default arguments for this command
    #[serde(rename = "default_args", default)]
    pub default_args: Option<Vec<String>>,
}

impl Config {
    /// Create a new configuration with default values
    pub fn new() -> Self {
        Self {
            edition: Edition::latest(),
            config_path: None,
            registry: RegistryConfig::default(),
            registries: HashMap::new(),
            command_defaults: Commands::default(),
        }
    }

    /// Create configuration from the workspace directory
    /// by locating the configuration file in the workspace.
    ///
    /// # Arguments
    /// * `workspace` - Path to the workspace directory
    ///
    /// # Returns
    /// Configuration loaded from file if found; or default configuration otherwise.
    pub fn new_from_workspace(workspace: &Path) -> miette::Result<Self> {
        let config_path = Self::locate_config(Some(workspace));
        match config_path {
            Some(config_path) => Self::new_from_config_file(&config_path),
            None => Ok(Default::default()),
        }
    }

    /// Create configuration from a TOML file
    ///
    /// # Arguments
    /// * `config_path` - Path to the configuration file
    pub fn new_from_config_file(config_path: &Path) -> miette::Result<Self> {
        let config_str = std::fs::read_to_string(config_path)
            .into_diagnostic()
            .wrap_err(miette!(
                "failed to read config file: {}",
                config_path.display()
            ))?;

        let raw_config: toml::Value =
            toml::from_str(&config_str)
                .into_diagnostic()
                .wrap_err(miette!(
                    "failed to parse config file: {}",
                    config_path.display()
                ))?;

        // Validate and parse the edition
        let edition = raw_config
            .get("edition")
            .and_then(|edition| edition.as_str())
            .ok_or_else(|| miette!("missing or invalid 'edition' field in config file"))?
            .into();

        match edition {
            Edition::Canary => (),
            _ => bail!(
                "unsupported config file edition '{}', supported editions: {}",
                Into::<&str>::into(edition),
                CANARY_EDITION
            ),
        }

        // Deserialize the remaining fields into the `Config` struct
        let mut config: Config =
            toml::from_str(&config_str)
                .into_diagnostic()
                .wrap_err(miette!(
                    "failed to parse configuration fields in file: {}",
                    config_path.display()
                ))?;

        // Set the edition and config path manually
        config.edition = edition;
        config.config_path = Some(config_path.to_path_buf());

        Ok(config)
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
    pub fn parse_registry_arg(&self, registry: &Option<String>) -> miette::Result<RegistryRef> {
        match registry {
            Some(registry) => registry.parse(),
            None => match &self.registry.default {
                Some(default_registry) => default_registry.parse(),
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
    pub(crate) fn lookup_registry(&self, name: &RegistryAlias) -> miette::Result<RegistryUri> {
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
            .get(command)
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
}

impl Default for Config {
    fn default() -> Self {
        Self::new()
    }
}

fn validate_edition<'de, D>(deserializer: D) -> Result<Edition, D::Error>
where
    D: Deserializer<'de>,
{
    let edition: String = Deserialize::deserialize(deserializer)?;
    match edition.as_str() {
        "0.10" | "canary" => Ok(Edition::from(edition.as_str())),
        _ => Err(de::Error::custom(format!(
            "unsupported config file edition '{}', supported editions: 0.10, canary",
            edition
        ))),
    }
}

impl Commands {
    /// Get the default arguments for a specific command
    ///
    /// # Arguments
    /// * `command` - The command name to get default arguments for, or None for global defaults
    ///
    /// # Returns
    /// A vector of default arguments for the specified command
    pub fn get(&self, command: Option<&str>) -> Option<&Vec<String>> {
        match command {
            Some(command) => self
                .specific
                .get(command)
                .and_then(|config| config.default_args.as_ref()),
            None => self.common.as_ref(),
        }
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
default_args = ["--generate-buf-yaml", "--generate-tonic-proto-module", "src/proto.rs"]
"#,
        )
        .unwrap();

        let alias: RegistryAlias = "acme".parse().unwrap();
        let config = Config::new_from_config_file(&config_path).unwrap();
        assert_eq!(config.edition, Edition::latest());
        assert_eq!(config.registry.default, Some("acme".to_string()));
        assert_eq!(
            config.registries.get(&alias).unwrap(),
            &"https://conan.acme.com/artifactory".parse().unwrap()
        );
        assert_eq!(
            config.command_defaults.get(Some("install")).unwrap(),
            &vec![
                "--generate-buf-yaml".to_string(),
                "--generate-tonic-proto-module".to_string(),
                "src/proto.rs".to_string()
            ]
        );
        assert_eq!(
            config.command_defaults.get(None).unwrap(),
            &vec!["--insecure".to_string()]
        );
    }

    #[test]
    fn test_new_from_empty_config_file() {
        let tmp_dir = TempDir::new().unwrap();
        let config_path = tmp_dir.path().join(CONFIG_FILE);
        std::fs::create_dir_all(config_path.parent().unwrap()).unwrap();
        let mut file = File::create(&config_path).unwrap();
        file.write_all(br#"edition = "0.10""#).unwrap();

        let config = Config::new_from_config_file(&config_path).unwrap();
        assert_eq!(config.edition, Edition::latest());
        assert_eq!(config.registry.default, None);
        assert!(config.registries.is_empty());
        assert!(config.command_defaults.common.is_none());
        assert!(config.command_defaults.specific.is_empty());
    }
}
