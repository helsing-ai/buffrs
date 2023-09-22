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

mod artifactory;
#[cfg(test)]
mod local;

pub use artifactory::Artifactory;
use eyre::ensure;
use serde::{Deserialize, Serialize};
use url::Url;

/// A `buffrs` registry used for remote package management
#[async_trait::async_trait]
pub trait Registry {
    /// Downloads a package from the registry
    async fn download(&self, dependency: Dependency) -> eyre::Result<Package>;
    /// Publishes a package to the registry
    async fn publish(&self, package: Package, repository: String) -> eyre::Result<()>;
}

/// An enum containing all supported registries
pub enum RegistryType {
    Artifactory,
}

#[derive(Debug, Clone, Hash, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct RegistryUri(Url);

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

impl FromStr for RegistryUri {
    type Err = eyre::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let slf = Self(Url::from_str(value)?);
        slf.sanity_check_url()?;
        Ok(slf)
    }
}

impl RegistryUri {
    pub fn sanity_check_url(&self) -> eyre::Result<()> {
        tracing::debug!(
            "checking that url begins with http or https: {}",
            self.0.scheme()
        );
        ensure!(
            self.0.scheme() == "http" || self.0.scheme() == "https",
            "The self.0 must start with http:// or https://"
        );

        if let Some(host) = self.0.host_str() {
            if host.ends_with(".jfrog.io") {
                tracing::debug!(
                    "checking that jfrog.io url ends with /artifactory: {}",
                    self.0.path()
                );
                ensure!(
                    self.0.path().ends_with("/artifactory"),
                    "The url must end with '/artifactory' when using a *.jfrog.io host"
                );
            }
        }

        Ok(())
    }
}
