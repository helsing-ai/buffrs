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

use miette::{miette, Context, Diagnostic, IntoDiagnostic};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, env, path::PathBuf};
use thiserror::Error;
use tokio::fs;

use crate::{
    errors::{DeserializationError, FileExistsError, ReadError, SerializationError, WriteError},
    registry::RegistryUri,
    ManagedFile,
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

const BUFFRS_HOME_VAR: &str = "BUFFRS_HOME";

#[derive(Error, Diagnostic, Debug)]
#[error("could not determine credentials location")]
struct LocateError(#[diagnostic_source] miette::Report);

fn location() -> Result<PathBuf, LocateError> {
    env::var(BUFFRS_HOME_VAR)
        .map(PathBuf::from)
        .or_else(|_| {
            home::home_dir()
                .ok_or_else(|| miette!("{BUFFRS_HOME_VAR} is not set and the user's home folder could not be determined"))
        })
        .map(|home| home.join(BUFFRS_HOME).join(CREDENTIALS_FILE)).map_err(LocateError)
}

impl Credentials {
    /// Checks if the credentials exists
    pub async fn exists() -> miette::Result<bool> {
        fs::try_exists(location().into_diagnostic()?)
            .await
            .into_diagnostic()
            .wrap_err(FileExistsError(CREDENTIALS_FILE))
    }

    /// Reads the credentials from the file system
    pub async fn read() -> miette::Result<Self> {
        let raw: RawCredentialCollection = toml::from_str(
            &fs::read_to_string(location().into_diagnostic()?)
                .await
                .into_diagnostic()
                .wrap_err(ReadError(CREDENTIALS_FILE))?,
        )
        .into_diagnostic()
        .wrap_err(DeserializationError(ManagedFile::Credentials))?;

        Ok(raw.into())
    }

    /// Writes the credentials to the file system
    pub async fn write(&self) -> miette::Result<()> {
        let location = location()?;

        if let Some(parent) = location.parent() {
            // if directory already exists, error is returned but that is fine
            fs::create_dir(parent).await.ok();
        }

        let data: RawCredentialCollection = self.clone().into();

        fs::write(
            location,
            toml::to_string(&data)
                .into_diagnostic()
                .wrap_err(SerializationError(ManagedFile::Credentials))?
                .into_bytes(),
        )
        .await
        .into_diagnostic()
        .wrap_err(WriteError(CREDENTIALS_FILE))
    }

    /// Loads the credentials from the file system
    ///
    /// Note: Initializes the credential file if its not present
    pub async fn load() -> miette::Result<Self> {
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
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
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
