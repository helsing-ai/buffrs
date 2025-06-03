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

#![warn(missing_docs)]
#![doc = include_str!("../README.md")]

use miette::Diagnostic;
use std::{env, path::PathBuf};
use thiserror::Error;

/// Caching implementation
pub mod cache;
/// CLI command implementations
pub mod command;
/// Configuration file (.buffrs/config.toml) handling
pub mod config;
/// Credential management
pub mod credentials;
/// Common error types
pub mod errors;
/// Integration with external tools
pub mod integration;
/// Lockfile implementation
pub mod lock;
/// Manifest format and IO
pub mod manifest;
/// Packages formats and utilities
pub mod package;
/// Supported registries
pub mod registry;
/// Resolve package dependencies.
pub mod resolver;
/// Validation for buffrs packages.
#[cfg(feature = "validation")]
pub mod validation;

/// Managed directory for `buffrs`
pub const BUFFRS_HOME: &str = ".buffrs";

pub(crate) const BUFFRS_HOME_VAR: &str = "BUFFRS_HOME";

#[derive(Error, Diagnostic, Debug)]
#[error("could not determine buffrs home location")]
struct HomeError(#[diagnostic_source] miette::Report);

fn home() -> Result<PathBuf, HomeError> {
    env::var(BUFFRS_HOME_VAR)
        .map(PathBuf::from)
        .or_else(|_| {
            home::home_dir()
                .ok_or_else(|| miette::miette!("{BUFFRS_HOME_VAR} is not set and the user's home folder could not be determined"))
        })
        .map(|home| home.join(BUFFRS_HOME))
        .map_err(HomeError)
}

#[derive(Debug)]
pub(crate) enum ManagedFile {
    Credentials,
    Manifest,
    Lock,
}

impl ManagedFile {
    fn name(&self) -> &str {
        use credentials::CREDENTIALS_FILE;
        use lock::LOCKFILE;
        use manifest::MANIFEST_FILE;

        match self {
            ManagedFile::Manifest => MANIFEST_FILE,
            ManagedFile::Lock => LOCKFILE,
            ManagedFile::Credentials => CREDENTIALS_FILE,
        }
    }
}

impl std::fmt::Display for ManagedFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}
