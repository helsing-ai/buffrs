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

use miette::{miette, Context, IntoDiagnostic};
use serde::{Deserialize, Serialize};
use std::{io::ErrorKind, path::PathBuf};
use tokio::fs;

use crate::{
    errors::{DeserializationError, FileExistsError, ReadError, SerializationError, WriteError},
    registry::RegistryTable,
    ManagedFile,
};

/// Filename of the config file
pub const CONFIG_FILE: &str = "config.toml";

/// Configuration file for the buffrs cli
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Config {
    /// A mapping from registry name to their corresponding uri
    pub registry: Option<RegistryTable>,
}

impl Config {
    fn location() -> PathBuf {
        PathBuf::from("./.buffrs/").join(CONFIG_FILE)
    }

    /// Checks if the credentials exists
    pub async fn exists() -> miette::Result<bool> {
        fs::try_exists(Self::location())
            .await
            .into_diagnostic()
            .wrap_err(FileExistsError(CONFIG_FILE))
    }

    /// Reads the credentials from the file system
    pub async fn read() -> miette::Result<Option<Self>> {
        // if the file does not exist, we don't need to treat it as an error.
        match fs::read_to_string(Self::location()).await {
            Ok(contents) => {
                let raw = toml::from_str(&contents)
                    .into_diagnostic()
                    .wrap_err(DeserializationError(ManagedFile::Configuration))?;

                Ok(Some(raw))
            }
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error)
                .into_diagnostic()
                .wrap_err(ReadError(CONFIG_FILE)),
        }
    }

    /// Writes the credentials to the file system
    pub async fn write(&self) -> miette::Result<()> {
        let location = Self::location();

        if let Some(parent) = location.parent() {
            // if directory already exists, error is returned but that is fine
            fs::create_dir(parent).await.ok();
        }

        fs::write(
            location,
            toml::to_string(&self)
                .into_diagnostic()
                .wrap_err(SerializationError(ManagedFile::Configuration))?
                .into_bytes(),
        )
        .await
        .into_diagnostic()
        .wrap_err(WriteError(CONFIG_FILE))
    }

    /// Loads the config from the file system, returning an error if it doesnt exist
    pub async fn load() -> miette::Result<Self> {
        Self::read()
            .await
            .transpose()
            .ok_or(miette!("missing configuration file"))?
    }
}
