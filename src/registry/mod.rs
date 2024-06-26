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
mod cache;

use crate::manifest::Dependency;
pub use artifactory::Artifactory;
use miette::{ensure, miette, Context, IntoDiagnostic};
use semver::VersionReq;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use url::Url;

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

impl FromStr for RegistryUri {
    type Err = miette::Report;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let url = Url::from_str(value)
            .into_diagnostic()
            .wrap_err(miette!("not a valid URL: {value}"))?;

        sanity_check_url(&url)?;

        Ok(Self(url))
    }
}

fn sanity_check_url(url: &Url) -> miette::Result<()> {
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
        Err(miette!("the URI must contain a host component: {url}"))
    }
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

    use super::RegistryUri;

    fn get_dependency(version: &str) -> Dependency {
        let registry = RegistryUri::from_str("https://my-registry.com").unwrap();
        let repository = String::from("my-repo");
        let package = PackageName::from_str("package").unwrap();
        let version = VersionReq::from_str(version).unwrap();
        Dependency::new(&registry, repository, package, version)
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
