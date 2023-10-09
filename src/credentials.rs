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
use std::{collections::HashMap, env, path::PathBuf};
use tokio::fs;

use crate::registry::RegistryUri;

/// Global configuration directory for `buffrs`
pub const BUFFRS_HOME: &str = ".buffrs";
/// Filename of the credential store
pub const CREDENTIALS_FILE: &str = "credentials.toml";

/// Credential store for storing authentication data
#[derive(Debug, Default, Clone)]
pub struct Credentials {
    /// A mapping from registry URIs to their corresponding tokens
    pub registry_tokens: HashMap<RegistryUri, String>,
}

impl Credentials {
    fn location() -> eyre::Result<PathBuf> {
        let home = home::home_dir().wrap_err("Failed to locate home directory")?;

        let home = env::var("BUFFRS_HOME").map(PathBuf::from).unwrap_or(home);

        Ok(home.join(BUFFRS_HOME).join(CREDENTIALS_FILE))
    }

    /// Checks if the credentials exists
    pub async fn exists() -> eyre::Result<bool> {
        fs::try_exists(Self::location()?)
            .await
            .wrap_err("Failed to detect credentials")
    }

    /// Reads the credentials from the file system
    pub async fn read() -> eyre::Result<Self> {
        let toml = fs::read_to_string(Self::location()?)
            .await
            .wrap_err("Failed to read credentials")?;

        let raw: RawCredentialCollection =
            toml::from_str(&toml).wrap_err("Failed to parse credentials")?;

        Ok(raw.into())
    }

    /// Writes the credentials to the file system
    pub async fn write(&self) -> eyre::Result<()> {
        fs::create_dir(
            Self::location()?
                .parent()
                .wrap_err("Invalid credentials location")?,
        )
        .await
        .ok();

        let data: RawCredentialCollection = self.clone().into();

        fs::write(Self::location()?, toml::to_string(&data)?.into_bytes())
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
