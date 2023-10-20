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
    ops::{Deref, DerefMut},
    str::FromStr,
};

mod artifactory;
#[cfg(test)]
mod local;

pub use artifactory::Artifactory;
use miette::{ensure, miette, IntoDiagnostic, Diagnostic};
use semver::VersionReq;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use url::Url;

use crate::manifest::Dependency;

/// A representation of a registry URI
#[derive(Debug, Clone, Hash, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct RegistryUri(Url);

impl From<RegistryUri> for Url {
    fn from(value: RegistryUri) -> Self {
        value.0
    }
}

impl Deref for RegistryUri {
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

/// Error parsing a registry URI
#[derive(Error, Debug, Diagnostic)]
pub enum RegistryUriError {
    /// URL parse error
    #[error("not a valid URL")]
    Parse(#[from] url::ParseError),

    /// Invalid scheme
    #[error("invalid scheme {0:}, must be http or https")]
    Scheme(String),

    /// Artifactory
    #[error("the url must end with '/artifactory' when using a *.jfrog.io host")]
    Artifactory,

    /// Missing host
    #[error("the URI must contain a host component")]
    MissingHost,
}

impl FromStr for RegistryUri {
    type Err = RegistryUriError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let url = Url::from_str(value)?;
        sanity_check_url(&url)?;
        Ok(Self(url))
    }
}

fn sanity_check_url(url: &Url) -> Result<(), RegistryUriError> {
    let scheme = url.scheme();

    if !["http", "https"].contains(&scheme) {
        return Err(RegistryUriError::Scheme(scheme.into()));
    }

    let Some(host) = url.host_str() else {
        return Err(RegistryUriError::MissingHost);
    };

    if host.ends_with(".jfrog.io") {
        if !url.path().ends_with("/artifactory") {
            return Err(RegistryUriError::Artifactory);
        }
    }

    Ok(())
}

#[derive(Error, Debug)]
#[error("{0} is not a supported version requirement")]
struct UnsupportedVersionRequirement(VersionReq);

fn dependency_version_string(dependency: &Dependency) -> miette::Result<String> {
    let version = dependency
        .manifest
        .version
        .comparators
        .first()
        .ok_or_else(|| UnsupportedVersionRequirement(dependency.manifest.version.clone()))
        .into_diagnostic()?;

    ensure!(
        version.op == semver::Op::Exact,
        UnsupportedVersionRequirement(dependency.manifest.version.clone(),)
    );

    let minor_version = version
        .minor
        .ok_or_else(|| miette!("version missing minor number"))?;

    let patch_version = version
        .patch
        .ok_or_else(|| miette!("version missing patch number"))?;

    Ok(format!(
        "{}.{}.{}{}",
        version.major,
        minor_version,
        patch_version,
        if version.pre.is_empty() {
            "".to_owned()
        } else {
            format!("-{}", version.pre)
        }
    ))
}
