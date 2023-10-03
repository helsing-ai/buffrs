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

use displaydoc::Display;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, env, path::PathBuf};
use thiserror::Error;
use tokio::fs;

use crate::{
    errors::{DeserializationError, SerializationError},
    registry::RegistryUri,
};

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

#[derive(Error, Display, Debug)]
#[non_exhaustive]
#[allow(missing_docs)]
pub enum LocateError {
    /// the home folder could not be determined
    MissingHome,
}

#[derive(Error, Display, Debug)]
#[non_exhaustive]
#[allow(missing_docs)]
pub enum ExistsError {
    /// could not locate the credentials file
    Locate(#[from] LocateError),
    /// could not access the filesystem
    Io(#[from] std::io::Error),
}

#[derive(Error, Display, Debug)]
#[non_exhaustive]
#[allow(missing_docs)]
pub enum ReadError {
    /// could not locate the credentials file
    Locate(#[from] LocateError),
    /// could not read the file
    Io(#[from] std::io::Error),
    /// could not deserialize the credentials
    Deserialize(#[from] DeserializationError),
}

#[derive(Error, Display, Debug)]
#[non_exhaustive]
#[allow(missing_docs)]
pub enum WriteError {
    /// could not locate the credentials file
    Locate(#[from] LocateError),
    /// could not write to the file
    Io(#[from] std::io::Error),
    /// could not serialize the credentials
    Serialize(#[from] SerializationError),
}

impl Credentials {
    fn location() -> Result<PathBuf, LocateError> {
        env::var("BUFFRS_HOME")
            .map(PathBuf::from)
            .or(home::home_dir().ok_or(LocateError::MissingHome))
            .map(|home| home.join(BUFFRS_HOME).join(CREDENTIALS_FILE))
    }

    /// Checks if the credentials exists
    pub async fn exists() -> Result<bool, ExistsError> {
        fs::try_exists(Self::location()?)
            .await
            .map_err(ExistsError::from)
    }

    /// Reads the credentials from the file system
    pub async fn read() -> Result<Self, ReadError> {
        let raw: RawCredentialCollection =
            toml::from_str(&fs::read_to_string(Self::location()?).await?)
                .map_err(DeserializationError::from)?;

        Ok(raw.into())
    }

    /// Writes the credentials to the file system
    pub async fn write(&self) -> Result<(), WriteError> {
        fs::create_dir(
            Self::location()
                .map_err(WriteError::Locate)?
                .parent()
                .expect("unexpected error: resolved credentials path has no parent"),
        )
        .await
        .ok();

        let data: RawCredentialCollection = self.clone().into();

        fs::write(
            Self::location()?,
            toml::to_string(&data)
                .map_err(SerializationError::from)?
                .into_bytes(),
        )
        .await
        .map_err(WriteError::from)
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
