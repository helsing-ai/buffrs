// (c) Copyright 2023 Helsing GmbH. All rights reserved.

use eyre::{Context, ContextCompat};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tokio::fs;

use crate::registry::ArtifactoryConfig;

/// Global configuration directory for `buffrs`
pub const BUFFRS_HOME: &str = ".buffrs";
/// Filename of the configuration
pub const CONFIG_FILE: &str = "config.toml";

/// Configuration format for storing authentication and settings
#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Config {
    /// Artifactory related configuration
    pub artifactory: Option<ArtifactoryConfig>,
}

impl Config {
    fn location() -> eyre::Result<PathBuf> {
        let home = home::home_dir().wrap_err("Failed to locate home directory")?;

        Ok(home.join(BUFFRS_HOME).join(CONFIG_FILE))
    }

    /// Checks if the configuration exists
    pub async fn exists() -> eyre::Result<bool> {
        fs::try_exists(Self::location()?)
            .await
            .wrap_err("Failed to detect config")
    }

    /// Reads the configuration from the file system
    pub async fn read() -> eyre::Result<Self> {
        let toml = fs::read_to_string(Self::location()?)
            .await
            .wrap_err("Failed to read manifest")?;

        toml::from_str(&toml).wrap_err("Failed to parse config")
    }

    /// Writes the configuration to the file system
    pub async fn write(&self) -> eyre::Result<()> {
        fs::create_dir(
            Self::location()?
                .parent()
                .wrap_err("Invalid config location")?,
        )
        .await
        .ok();

        fs::write(Self::location()?, toml::to_string(&self)?.into_bytes())
            .await
            .wrap_err("Failed to write config")
    }

    /// Loads the configuration from the file system
    ///
    /// Note: Initializes the configuration if its not present
    pub async fn load() -> eyre::Result<Self> {
        let Ok(cfg) = Self::read().await else {
            let cfg = Config::default();
            cfg.write().await?;
            return Ok(cfg);
        };

        Ok(cfg)
    }
}
