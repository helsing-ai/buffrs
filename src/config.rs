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

use crate::registry::RegistryUri;
use miette::{bail, ensure, miette, Context, IntoDiagnostic};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    str::FromStr,
};

/// Representation of the .buffrs/config.toml configuration file
///
/// # Example
///
/// ```toml
/// [registries]
/// some_org = "https://artifactory.example.com/artifactory/some-org"
///
/// [registry]
/// default = "some_org"
/// ```
///
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// Path to the configuration file
    config_path: Option<PathBuf>,

    /// Default registry to use if none is specified
    default_registry: Option<String>,

    /// List of registries
    registries: HashMap<String, RegistryUri>,
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
                config_path: None,
                default_registry: None,
                registries: HashMap::new(),
            }),
        }
    }

    /// Resolve the registry URI from the configuration
    ///
    /// # Arguments
    /// * `registry` - The registry name or URI to resolve
    ///
    /// # Returns
    /// The resolved registry URI
    pub fn registry_or_default(&self, registry: &Option<String>) -> miette::Result<RegistryUri> {
        match registry {
            Some(registry) => {
                match RegistryUri::from_str(registry) {
                    Ok(uri) => Ok(uri),
                    Err(_) => self.lookup_registry(registry),
                }
            }
            None => match &self.default_registry {
                Some(default_registry) => self
                    .registries
                    .get(default_registry)
                    .cloned()
                    .ok_or_else(|| {
                        miette!("no registry provided (using --registry) and no default registry in .buffrs/config.toml")
                    }),
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
    pub fn lookup_registry(&self, name: &str) -> miette::Result<RegistryUri> {
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

    /// Locate the configuration file in the current directory or any parent directories
    ///
    /// # Arguments
    /// * `cwd` - Starting directory to search for the configuration file
    ///
    /// # Returns
    /// Some(PathBuf) if the configuration file is found, None otherwise
    fn locate_config(cwd: Option<&Path>) -> Option<PathBuf> {
        if let Some(cwd) = cwd {
            let mut current_dir = cwd.to_owned();

            loop {
                let config_path = current_dir.join(".buffrs/config.toml");
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
        let config = std::fs::read_to_string(config_path)
            .into_diagnostic()
            .wrap_err(miette!(
                "failed to read config file: {}",
                config_path.display()
            ))?;
        let config: toml::Value = toml::from_str(&config).into_diagnostic().wrap_err(miette!(
            "failed to parse config file: {}",
            config_path.display()
        ))?;

        // Load registries from [registries] section
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
                        Ok((name.to_string(), RegistryUri::from_str(uri)?))
                    })
                    .collect::<miette::Result<HashMap<String, RegistryUri>>>()
            })
            .unwrap_or_else(|| Ok(HashMap::new()))
            .wrap_err(miette!(
                "failed to load registries from config file: {}",
                config_path.display()
            ))?;

        // Locate default registry from [registry.default]
        let default_registry = config
            .get("registry")
            .and_then(|registry| registry.get("default"))
            .and_then(|default| default.as_str())
            .map(|default| default.to_string());

        // Ensure that the default registry is in the list of registries
        if let Some(ref default_registry) = default_registry {
            ensure!(
                registries.contains_key(default_registry),
                "default registry '{}' not found in list of registries",
                default_registry
            );
        }

        Ok(Self {
            config_path: Some(config_path.to_owned()),
            default_registry,
            registries,
        })
    }
}
