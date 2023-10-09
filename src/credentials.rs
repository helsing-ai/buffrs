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

use eyre::{Context, ContextCompat};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    env::{var, VarError},
    io::ErrorKind,
    path::PathBuf,
};
use tokio::fs;

use crate::registry::RegistryUri;

/// Global configuration directory for `buffrs`
pub const BUFFRS_HOME: &str = ".buffrs";
pub const BUFFRS_DIR: &str = "buffrs";
/// Filename of the credential store
pub const CREDENTIALS_FILE: &str = "credentials.toml";

/// Credential store for storing authentication data
#[derive(Debug, Default, Clone)]
pub struct Credentials {
    /// A mapping from registry URIs to their corresponding tokens
    pub registry_tokens: HashMap<RegistryUri, String>,
}

impl Credentials {
    /// Get buffrs config directory.
    fn buffrs_home() -> Result<PathBuf, VarError> {
        var("BUFFRS_HOME").map(PathBuf::from)
    }

    /// XDG path: uses whichever config directory is appropriate for the platform.
    ///
    /// For example, on Linux this might be `~/.config/buffrs/credentials.toml`.
    fn xdg_path() -> PathBuf {
        Self::buffrs_home()
            .unwrap_or_else(|_| dirs::config_dir().expect("get config dir").join(BUFFRS_DIR))
            .join(CREDENTIALS_FILE)
    }

    /// Legacy path: hard-coded to `~/.buffrs/credentials.toml`.
    fn legacy_path() -> PathBuf {
        Self::buffrs_home()
            .unwrap_or_else(|_| dirs::home_dir().expect("get home dir").join(BUFFRS_HOME))
            .join(CREDENTIALS_FILE)
    }

    /// Possible locations for the credentials file
    fn possible_locations() -> Vec<PathBuf> {
        vec![Self::xdg_path(), Self::legacy_path()]
    }

    /// Checks if the credentials exists
    pub async fn exists() -> eyre::Result<bool> {
        for location in &Self::possible_locations() {
            if fs::try_exists(&location).await? {
                return Ok(true);
            }
        }

        return Ok(false);
    }

    /// Reads the credentials from the file system
    pub async fn read() -> eyre::Result<Self> {
        for location in &Self::possible_locations() {
            let contents = match fs::read_to_string(location).await {
                Ok(string) => string,
                Err(error) if error.kind() == ErrorKind::NotFound => continue,
                Err(error) => return Err(error).wrap_err("opening credentials file"),
            };

            let raw: RawCredentialCollection =
                toml::from_str(&contents).wrap_err("Failed to parse credentials")?;

            return Ok(raw.into());
        }

        eyre::bail!("cannot parse credentials")
    }

    /// Writes the credentials to the file system
    pub async fn write(&self) -> eyre::Result<()> {
        let location = Self::xdg_path();

        fs::create_dir(location.parent().wrap_err("Invalid credentials location")?)
            .await
            .ok();

        let data: RawCredentialCollection = self.clone().into();

        fs::write(location, toml::to_string(&data)?.into_bytes())
            .await
            .wrap_err("Failed to write credentials")
    }

    /// Loads the credentials from the file system, or creates default if not present.
    pub async fn load_or_init() -> eyre::Result<Self> {
        let Ok(credentials) = Self::read().await else {
            let credentials = Credentials::default();
            credentials.write().await?;
            return Ok(credentials);
        };

        Ok(credentials)
    }
}

/// Credential store for storing authentication data. Serialization type.
#[derive(Serialize, Deserialize)]
struct RawCredentialCollection {
    credentials: Vec<RawRegistryCredentials>,
}

/// Credentials for a single registry. Serialization type.
#[derive(Serialize, Deserialize)]
struct RawRegistryCredentials {
    uri: RegistryUri,
    token: String,
}

impl From<RawCredentialCollection> for Credentials {
    fn from(value: RawCredentialCollection) -> Self {
        let credentials = value
            .credentials
            .into_iter()
            .map(|it| (it.uri, it.token))
            .collect();

        Self {
            registry_tokens: credentials,
        }
    }
}

impl From<Credentials> for RawCredentialCollection {
    fn from(value: Credentials) -> Self {
        let credentials = value
            .registry_tokens
            .into_iter()
            .map(|(uri, token)| RawRegistryCredentials { uri, token })
            .collect();

        Self { credentials }
    }
}
