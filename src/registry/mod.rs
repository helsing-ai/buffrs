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
    collections::HashMap,
    fmt::{self, Display},
    str::FromStr,
};

mod artifactory;
#[cfg(test)]
mod cache;

pub use artifactory::Artifactory;
use miette::{ensure, miette, Context, IntoDiagnostic};
use semver::VersionReq;
use serde::{de, Deserialize, Deserializer, Serialize};
use serde_with::{DeserializeFromStr, SerializeDisplay};
use thiserror::Error;
use url::Url;

use crate::manifest::Dependency;

/// A registry lookup table to retrieve actual registry uris from
///
/// Must contain at least the default registry
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RegistryTable {
    default: RegistryUri,
    named: HashMap<String, RegistryUri>,
}

impl RegistryTable {
    pub fn get(&self, reg: &Registry) -> Option<&RegistryUri> {
        match reg {
            &Registry::Default => Some(&self.default),
            &Registry::Named(name) => self.named.get(&name),
        }
    }
}

impl<'de> Deserialize<'de> for RegistryTable {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let mut named = HashMap::deserialize(deserializer)?;

        let default = named
            .remove("default")
            .ok_or_else(|| de::Error::missing_field("default"))?;

        Ok(Self { default, named })
    }
}

/// A pointer to a registry in the registry table
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(into = "&str", from = "&str")]
pub enum Registry {
    /// Use the registry configured as default registry
    Default,
    /// Use a named registry that is configured in the configuration
    Named(String),
}

impl Registry {
    const DEFAULT: &'static str = "default";

    pub fn is_default(&self) -> bool {
        matches!(self, &Self::Default)
    }
}

impl Default for Registry {
    fn default() -> Self {
        Self::Default
    }
}

impl From<Registry> for &str {
    fn from(value: Registry) -> Self {
        match value {
            Registry::Default => &Registry::DEFAULT,
            Registry::Named(v) => &v,
        }
    }
}

impl From<&str> for Registry {
    fn from(value: &str) -> Self {
        if value == Self::DEFAULT {
            return Self::Default;
        }

        Self::Named(value.to_owned())
    }
}

impl Display for Registry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let fmt = match self {
            &Registry::Default => Registry::DEFAULT,
            &Registry::Named(ref v) => v,
        };

        write!(f, "{fmt}")
    }
}

/// A representation of a artifactory registry URI
///
/// This is compatible with everything in the format `<org>.jfrog.io/artifactory/<repository>`
#[derive(
    Debug, Clone, Hash, SerializeDisplay, DeserializeFromStr, PartialEq, Eq, PartialOrd, Ord,
)]
pub struct RegistryUri {
    raw: Url,
    repository: String,
}

impl RegistryUri {
    /// Get the base path of the artifactory instance
    pub fn base(&self) -> Url {
        let mut url = self.raw.clone();

        url.set_path("/artifactory");

        url
    }
}

impl From<RegistryUri> for Url {
    fn from(value: RegistryUri) -> Self {
        value.raw
    }
}

impl Display for RegistryUri {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.raw)
    }
}

impl FromStr for RegistryUri {
    type Err = miette::Report;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let url = Url::from_str(value)
            .into_diagnostic()
            .wrap_err(miette!("not a valid URL: {value}"))?;

        let (raw, repository) = inspect_url(url)?;

        Ok(Self { raw, repository })
    }
}

fn inspect_url(url: Url) -> miette::Result<(Url, String)> {
    let scheme = url.scheme();

    ensure!(
        scheme == "http" || scheme == "https",
        "invalid URI scheme {scheme} - must be http or https"
    );

    ensure!(
        url.has_host(),
        "the URI must contain a host component: {url}"
    );

    let mut path = url
        .path_segments()
        .ok_or(miette!("the URI must contain a path"))?;

    ensure!(
        path.next() == Some("artifactory"),
        "expecting URI in the format of `<host>/artifactory/<repository>`"
    );

    let repository = path.next().ok_or(miette!(
        "registry URI is missing the repository component: {url}"
    ))?;

    Ok((url, repository.to_string()))
}

#[derive(Error, Debug)]
#[error("{0} is not a supported version requirement")]
struct UnsupportedVersionRequirement(VersionReq);

#[derive(Error, Debug)]
#[error("{0} is not supported yet. Pin the exact version you want to use with '='. For example: '=1.0.4' instead of '^1.0.0'")]
struct VersionNotPinned(VersionReq);

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
        VersionNotPinned(dependency.manifest.version.clone(),)
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

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use semver::VersionReq;

    use crate::{
        manifest::Dependency,
        package::PackageName,
        registry::{dependency_version_string, VersionNotPinned},
    };

    use super::{Registry, RegistryUri};

    fn get_dependency(version: &str) -> Dependency {
        let package = PackageName::from_str("package").unwrap();
        let version = VersionReq::from_str(version).unwrap();
        Dependency::new(Registry::Default, package, version)
    }

    #[test]
    fn valid_version() {
        let dependency = get_dependency("=0.0.1");
        assert!(dependency_version_string(&dependency).is_ok_and(|version| version == "0.0.1"));

        let dependency = get_dependency("=0.0.1-23");
        assert!(dependency_version_string(&dependency).is_ok_and(|version| version == "0.0.1-23"));

        let dependency = get_dependency("=0.0.1-ab");
        assert!(dependency_version_string(&dependency).is_ok_and(|version| version == "0.0.1-ab"));
    }

    #[test]
    fn unsupported_version_operator() {
        let dependency = get_dependency("^0.0.1");
        assert!(
            dependency_version_string(&dependency).is_err_and(|err| err.is::<VersionNotPinned>())
        );

        let dependency = get_dependency("~0.0.1");
        assert!(
            dependency_version_string(&dependency).is_err_and(|err| err.is::<VersionNotPinned>())
        );

        let dependency = get_dependency("<=0.0.1");
        assert!(
            dependency_version_string(&dependency).is_err_and(|err| err.is::<VersionNotPinned>())
        );
    }

    #[test]
    fn incomplete_version() {
        let dependency = get_dependency("=1.0");
        assert!(dependency_version_string(&dependency).is_err());

        let dependency = get_dependency("=1");
        assert!(dependency_version_string(&dependency).is_err());
    }
}
