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

mod artifactory;
#[cfg(test)]
mod local;

pub use artifactory::Artifactory;
use eyre::{ensure, eyre, Context};
use serde::{Deserialize, Serialize};
use url::Url;

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

impl FromStr for RegistryUri {
    type Err = eyre::Report;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let url = Url::from_str(value).wrap_err_with(|| format!("not a valid URL: {value}"))?;
        sanity_check_url(&url)?;
        Ok(Self(url))
    }
}

fn sanity_check_url(url: &Url) -> eyre::Result<()> {
    let scheme = url.scheme();
    ensure!(
        scheme == "http" || scheme == "https",
        "invalid URI scheme {scheme} - must be http or https"
    );

    if let Some(host) = url.host_str() {
        ensure!(
            !host.ends_with(".jfrog.io") || url.path().ends_with("/artifactory"),
            "the url must end with '/artifactory' when using a *.jfrog.io host"
        );
        Ok(())
    } else {
        Err(eyre!("the URI must contain a host component: {url}"))
    }
}
