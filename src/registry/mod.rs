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
    str::FromStr,
};

mod artifactory;
#[cfg(test)]
mod cache;

use crate::manifest::DependencyManifest;
use crate::{config, manifest::Dependency};
pub use artifactory::{Artifactory, CertValidationPolicy};
use miette::{ensure, miette, IntoDiagnostic};
use semver::VersionReq;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use url::Url;

/// A representation of a registry URI
#[derive(Debug, Clone, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub enum RegistryUri {
    /// A URL to a registry
    Url(Url),
    /// An alias to a registry
    Alias(String),
    /// A resolved alias to a registry
    ResolvedAlias {
        /// The alias
        alias: String,
        /// The resolved URL
        url: Url,
    },
}

impl RegistryUri {
    /// Get the raw URL of the registry with any alias resolved
    ///
    /// # Arguments
    /// * `config` - The configuration to use to resolve the alias
    pub fn with_alias_resolved(&self, config: Option<&config::Config>) -> miette::Result<Self> {
        match self {
            RegistryUri::Alias(alias) => match config {
                Some(config) => {
                    let url = config.lookup_registry(alias)?;
                    Ok(RegistryUri::ResolvedAlias {
                        alias: alias.clone(),
                        url: url.clone(),
                    })
                }
                None => Err(miette!("no configuration provided to resolve alias")),
            },
            _ => Ok(self.clone()),
        }
    }

    /// Serializer for resolved RegistryUris
    pub fn serialize_resolved<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            RegistryUri::ResolvedAlias { url, .. } => url.serialize(serializer),
            RegistryUri::Url(url) => url.serialize(serializer),
            _ => Err(serde::ser::Error::custom(
                "cannot serialize unresolved alias",
            )),
        }
    }
}

impl<'de> Deserialize<'de> for RegistryUri {
    fn deserialize<D>(deserializer: D) -> Result<RegistryUri, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let url = String::deserialize(deserializer)?;
        RegistryUri::from_str(&url).map_err(serde::de::Error::custom)
    }
}

impl Serialize for RegistryUri {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.to_string().serialize(serializer)
    }
}

impl TryFrom<RegistryUri> for Url {
    type Error = miette::Report;

    fn try_from(value: RegistryUri) -> Result<Self, Self::Error> {
        match value {
            RegistryUri::Url(url) => Ok(url),
            RegistryUri::ResolvedAlias { url, .. } => Ok(url),
            _ => Err(miette!(
                "cannot convert unresolved alias \"{value}\" to URL"
            )),
        }
    }
}

impl TryFrom<&RegistryUri> for Url {
    type Error = miette::Report;

    fn try_from(value: &RegistryUri) -> Result<Self, Self::Error> {
        // Delegate to the implementation for the owned type
        TryFrom::<RegistryUri>::try_from(value.clone())
    }
}

impl Display for RegistryUri {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RegistryUri::Url(url) => write!(f, "{}", url),
            RegistryUri::Alias(alias) => write!(f, "{}", alias),
            RegistryUri::ResolvedAlias { alias, url } => write!(f, "{} ({})", alias, url),
        }
    }
}

impl FromStr for RegistryUri {
    type Err = miette::Report;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        // If the string doesn't parse as a URL, use the custom "alias" scheme, with the value as the path
        match Url::from_str(value) {
            Ok(url) => {
                sanity_check_url(&url)?;
                Ok(Self::Url(url))
            }
            Err(_) => Ok(Self::Alias(value.to_owned())),
        }
    }
}

/// Ensure that the URL is valid for a registry
///
/// A valid registry URL must:
/// - Have a scheme of either "http" or "https"
/// - End with "/artifactory" if the host is a JFrog Artifactory instance
/// - Have a host component
///
/// # Arguments
/// * `url` - The URL to check
pub fn sanity_check_url(url: &Url) -> miette::Result<()> {
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
    let DependencyManifest::Remote(ref manifest) = dependency.manifest else {
        return Err(miette!(
            "unable to serialize version of local dependency ({})",
            dependency.package
        ));
    };

    let version = manifest
        .version
        .comparators
        .first()
        .ok_or_else(|| UnsupportedVersionRequirement(manifest.version.clone()))
        .into_diagnostic()?;

    ensure!(
        version.op == semver::Op::Exact,
        VersionNotPinned(manifest.version.clone())
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
