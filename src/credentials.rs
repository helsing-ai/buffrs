// (c) Copyright 2023 Helsing GmbH. All rights reserved.

use eyre::{Context, ContextCompat};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
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
