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
use std::{env, path::PathBuf};
use tokio::fs;

use crate::registry::ArtifactoryConfig;

/// Global configuration directory for `buffrs`
pub const BUFFRS_HOME: &str = ".buffrs";
/// Filename of the credential store
pub const CREDENTIALS_FILE: &str = "credentials.toml";

/// Credential store for storing authentication data
#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Credentials {
    /// Artifactory credentials
    pub artifactory: Option<ArtifactoryConfig>,
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

        toml::from_str(&toml).wrap_err("Failed to parse credentials")
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

        fs::write(Self::location()?, toml::to_string(&self)?.into_bytes())
            .await
            .wrap_err("Failed to write credentials")
    }

    /// Loads the credentials from the file system
    ///
    /// Note: Initializes the credential file if its not present
    pub async fn load() -> eyre::Result<Self> {
        let Ok(credentials) = Self::read().await else {
            let credentials = Credentials::default();
            credentials.write().await?;
            return Ok(credentials);
        };

        Ok(credentials)
    }
}
