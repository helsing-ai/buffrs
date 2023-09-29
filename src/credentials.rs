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

use serde::{Deserialize, Serialize};
use std::{collections::HashMap, env, path::PathBuf};
use thiserror::Error;
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

/// Error produced when locating the credentials file
#[derive(Error, Debug)]
pub enum LocateError {
    /// The home folder where the credentials file is stored could not be determined.
    ///
    /// This implies that BUFFRS_HOME is not set and that the user's home directory
    /// could not be detected.
    #[error("BUFFRS_HOME unset and could not resolve user's home directory")]
    MissingHome,
}

/// Error produced when checking if the credentials file exists
#[derive(Error, Debug)]
pub enum ExistsError {
    /// Could not locate the credentials file
    #[error("Failed to determine credentials location. {0}")]
    Locate(LocateError),
    /// Could not access the filesystem
    #[error("IO error: {0}")]
    Io(std::io::Error),
}

/// Error produced when loading the credentials file
#[derive(Error, Debug)]
pub enum ReadError {
    /// Could not locate the credentials file
    #[error("Failed to determine credentials location. {0}")]
    Locate(LocateError),
    /// Could not read the file from the filesystem
    #[error("IO error: {0}")]
    Io(std::io::Error),
    /// Could not parse the TOML credentials data
    #[error("Failed to deserialize credentials. Cause: {0}")]
    Toml(toml::de::Error),
}

/// Error produced when updating the credentials file
#[derive(Error, Debug)]
pub enum WriteError {
    /// Could not locate the credentials file
    #[error("Failed to determine credentials location. {0}")]
    Locate(LocateError),
    /// Could not write to the credentials file
    #[error("IO error: {0}")]
    Io(std::io::Error),
    /// Could not encode the credentials data as TOML
    #[error("Failed to serialize credentials. Cause: {0}")]
    Toml(toml::ser::Error),
}

impl Credentials {
    fn location() -> Result<PathBuf, LocateError> {
        let home = env::var("BUFFRS_HOME")
            .map(PathBuf::from)
            .or(home::home_dir().ok_or(LocateError::MissingHome))?;

        Ok(home.join(BUFFRS_HOME).join(CREDENTIALS_FILE))
    }

    /// Checks if the credentials exists
    pub async fn exists() -> Result<bool, ExistsError> {
        fs::try_exists(Self::location().map_err(ExistsError::Locate)?)
            .await
            .map_err(ExistsError::Io)
    }

    /// Reads the credentials from the file system
    pub async fn read() -> Result<Self, ReadError> {
        let toml = fs::read_to_string(Self::location().map_err(ReadError::Locate)?)
            .await
            .map_err(ReadError::Io)?;

        let raw: RawCredentialCollection = toml::from_str(&toml).map_err(ReadError::Toml)?;

        Ok(raw.into())
    }

    /// Writes the credentials to the file system
    pub async fn write(&self) -> Result<(), WriteError> {
        fs::create_dir(
            Self::location()
                .map_err(WriteError::Locate)?
                .parent()
                .expect("Unexpected error: resolved credentials path has no parent"),
        )
        .await
        .ok();

        let data: RawCredentialCollection = self.clone().into();

        fs::write(
            Self::location().map_err(WriteError::Locate)?,
            toml::to_string(&data)
                .map_err(WriteError::Toml)?
                .into_bytes(),
        )
        .await
        .map_err(WriteError::Io)
    }

    /// Loads the credentials from the file system
    ///
    /// Note: Initializes the credential file if its not present
    pub async fn load() -> Result<Self, WriteError> {
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
