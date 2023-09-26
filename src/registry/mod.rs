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

mod artifactory;

#[cfg(test)]
mod local;

use crate::{credentials::Credentials, manifest::Dependency, package::Package};
pub use artifactory::Artifactory;
use eyre::ensure;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fmt::{self, Display},
    ops::DerefMut,
    str::FromStr,
    sync::Arc,
};
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

impl FromStr for RegistryUri {
    type Err = eyre::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let slf = Self(Url::from_str(value)?);
        slf.sanity_check_url()?;
        Ok(slf)
    }
}

impl RegistryUri {
    fn sanity_check_url(&self) -> eyre::Result<()> {
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

    /// Returns the type of registry this URI refers to
    pub fn kind(&self) -> RegistryType {
        RegistryType::Artifactory
    }
}

#[derive(Default)]
pub struct RegistryProvider {
    credentials: Credentials,
    registries: HashMap<RegistryUri, Arc<dyn Registry + Send + Sync>>,
}

impl RegistryProvider {
    pub fn new(credentials: Credentials) -> Self {
        Self {
            credentials,
            registries: Default::default(),
        }
    }

    pub fn get_or_create(
        &mut self,
        registry_uri: &RegistryUri,
    ) -> eyre::Result<Arc<dyn Registry + Send + Sync>> {
        if !self.registries.contains_key(registry_uri) {
            let registry = Arc::new(match registry_uri.kind() {
                RegistryType::Artifactory => {
                    Artifactory::from_credentials(registry_uri, &self.credentials)?
                }
            });

            self.registries.insert(registry_uri.clone(), registry);
        }

        Ok(self.registries.get(registry_uri).unwrap().clone())
    }
}
