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

use std::{
    fmt::{self, Display},
    ops::DerefMut,
    str::FromStr,
};

use crate::{manifest::Dependency, package::Package};

pub mod artifactory;
#[cfg(test)]
mod local;

pub use artifactory::Artifactory;
use semver::VersionReq;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use url::Url;

#[derive(Error, Debug)]
pub enum DownloadError {
    #[error("{0} is not a supported version requirement")]
    UnsupportedVersionRequirement(VersionReq),
    #[error("Download request failed - reason: {0}")]
    RequestFailed(String),
    #[error("Invalid server response. Cause: {0}")]
    InvalidResponse(String),
}

#[derive(Error, Debug)]
pub enum PublishError {
    #[error("Publish request failed - reason: {0}")]
    RequestFailed(String),
}

/// A `buffrs` registry used for remote package management
#[async_trait::async_trait]
pub trait Registry {
    /// Downloads a package from the registry
    async fn download(&self, dependency: Dependency) -> Result<Package, DownloadError>;
    /// Publishes a package to the registry
    async fn publish(&self, package: Package, repository: String) -> Result<(), PublishError>;
}

#[derive(Debug, Clone, Hash, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct RegistryUri(Url);

impl From<RegistryUri> for Url {
    fn from(value: RegistryUri) -> Self {
        value.0
    }
}

impl std::ops::Deref for RegistryUri {
    type Target = Url;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for RegistryUri {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl Display for RegistryUri {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Error, Debug)]
pub enum UriValidationError {
    #[error("Not a valid URL: {0}")]
    InvalidUrl(String),
    #[error("Invalid URI scheme {0} - must be http or https")]
    InvalidScheme(String),
    #[error("The URI must contain a host: {0}")]
    MissingHost(Url),
    #[error("The url must end with '/artifactory' when using a *.jfrog.io host")]
    InvalidSuffix,
}

impl FromStr for RegistryUri {
    type Err = UriValidationError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let url = Url::from_str(value).map_err(|_| UriValidationError::InvalidUrl(value.into()))?;
        sanity_check_url(&url)?;
        Ok(Self(url))
    }
}

fn sanity_check_url(url: &Url) -> Result<(), UriValidationError> {
    let scheme = url.scheme();
    if scheme != "http" && scheme != "https" {
        return Err(UriValidationError::InvalidScheme(scheme.into()));
    }

    if let Some(host) = url.host_str() {
        if host.ends_with(".jfrog.io") && !url.path().ends_with("/artifactory") {
            return Err(UriValidationError::InvalidSuffix);
        }
    } else {
        return Err(UriValidationError::MissingHost(url.clone()));
    }

    Ok(())
}
