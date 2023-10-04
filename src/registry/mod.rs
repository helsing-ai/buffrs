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

use crate::{errors::HttpError, package::DecodePackageError};

mod artifactory;
#[cfg(test)]
mod local;

pub use artifactory::Artifactory;
use displaydoc::Display;
use semver::VersionReq;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use url::Url;

#[derive(Display, Error, Debug)]
#[non_exhaustive]
#[allow(missing_docs)]
pub enum DownloadError {
    /// {0} is not a supported version requirement
    UnsupportedVersionRequirement(VersionReq),
    /// http error downloading a dependency
    Http(#[from] HttpError),
    /// failed to decode downloaded package
    DecodePackage(#[from] DecodePackageError),
}

#[derive(Error, Display, Debug)]
#[non_exhaustive]
#[allow(missing_docs)]
pub enum PublishError {
    /// http error uploading a dependency
    Http(#[from] HttpError),
}

/// A representation of a registry URI
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

/// Error produced when a URI fails to be validated
#[derive(Display, Error, Debug)]
#[non_exhaustive]
pub enum UriValidationError {
    /// Not a valid URL: {0}
    InvalidUrl(String),
    /// Invalid URI scheme {0} - must be http or https
    InvalidScheme(String),
    /// The URI must contain a host: {0}
    MissingHost(Url),
    /// The url must end with '/artifactory' when using a *.jfrog.io host
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
            Err(UriValidationError::InvalidSuffix)
        } else {
            Ok(())
        }
    } else {
        Err(UriValidationError::MissingHost(url.clone()))
    }
}
