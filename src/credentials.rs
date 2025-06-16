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

use miette::{Context, IntoDiagnostic};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, io::ErrorKind, path::PathBuf};
use tokio::fs;

use crate::{
    ManagedFile,
    errors::{DeserializationError, FileExistsError, ReadError, SerializationError, WriteError},
    registry::RegistryUri,
};

/// Filename of the credential store
pub const CREDENTIALS_FILE: &str = "credentials.toml";

/// Credential store for storing authentication data
///
/// This type represents a snapshot of the read credential store.
#[derive(Debug, Default, Clone)]
pub struct Credentials {
    /// A mapping from registry URIs to their corresponding tokens
    pub registry_tokens: HashMap<RegistryUri, String>,
}

impl Credentials {
    fn location() -> miette::Result<PathBuf> {
        Ok(crate::home().into_diagnostic()?.join(CREDENTIALS_FILE))
    }

    /// Checks if the credentials exists
    pub async fn exists() -> miette::Result<bool> {
        fs::try_exists(Self::location()?)
            .await
            .into_diagnostic()
            .wrap_err(FileExistsError(CREDENTIALS_FILE))
    }

    /// Reads the credentials from the file system
    pub async fn read() -> miette::Result<Option<Self>> {
        // if the file does not exist, we don't need to treat it as an error.
        match fs::read_to_string(Self::location()?).await {
            Ok(contents) => {
                let raw: RawCredentialCollection = toml::from_str(&contents)
                    .into_diagnostic()
                    .wrap_err(DeserializationError(ManagedFile::Credentials))?;
                Ok(Some(raw.into()))
            }
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error)
                .into_diagnostic()
                .wrap_err(ReadError(CREDENTIALS_FILE)),
        }
    }

    /// Writes the credentials to the file system
    pub async fn write(&self) -> miette::Result<()> {
        let location = Self::location()?;

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

    /// Loads the credentials from the file system, returning default credentials if
    /// they do not exist.
    ///
    /// Note, this should not create files in the user's home directory, as we should
    /// not be performing global stateful operations in absence of a user instruction.
    pub async fn load() -> miette::Result<Self> {
        Ok(Self::read().await?.unwrap_or_else(Credentials::default))
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
