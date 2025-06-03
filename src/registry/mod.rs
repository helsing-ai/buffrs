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

use crate::{config, manifest::Dependency};
use crate::{errors::RegistryNameError, manifest::DependencyManifest};
pub use artifactory::{Artifactory, CertValidationPolicy};
use miette::{ensure, miette, Context, IntoDiagnostic};
use semver::VersionReq;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use url::Url;

/// Representation of a registry name
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct RegistryAlias(String);

impl FromStr for RegistryAlias {
    type Err = RegistryNameError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
        {
            Ok(RegistryAlias(s.to_string()))
        } else {
            Err(RegistryNameError)
        }
    }
}

impl Display for RegistryAlias {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A representation of a registry URI
#[derive(Debug, Clone, Hash, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct RegistryUri(Url);

impl RegistryUri {
    /// Get the host component of the registry URI
    pub fn host(&self) -> Option<&str> {
        self.0.host_str()
    }

    /// Get the path component of the registry URI
    pub fn path(&self) -> &str {
        self.0.path()
    }
}

/// A reference to a registry
#[derive(Debug, Clone, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub enum RegistryRef {
    /// A URL to a registry
    Url(RegistryUri),
    /// An alias to a registry
    Alias(RegistryAlias),
    /// A resolved alias to a registry
    ResolvedAlias {
        /// The alias
        alias: RegistryAlias,
        /// The resolved URL
        url: RegistryUri,
    },
}

impl RegistryRef {
    /// Get the raw URL of the registry with any alias resolved
    ///
    /// # Arguments
    /// * `config` - The configuration to use to resolve the alias
    pub fn with_alias_resolved(&self, config: Option<&config::Config>) -> miette::Result<Self> {
        match self {
            RegistryRef::Alias(alias) => match config {
                Some(config) => {
                    let url = config.lookup_registry(alias)?;
                    Ok(RegistryRef::ResolvedAlias {
                        alias: alias.clone(),
                        url: url.clone(),
                    })
                }
                None => Err(miette!(
                    "no configuration provided to resolve alias \"{}\"",
                    alias.0
                )),
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
            RegistryRef::ResolvedAlias { url, .. } => url.serialize(serializer),
            RegistryRef::Url(url) => url.serialize(serializer),
            _ => Err(serde::ser::Error::custom(
                "cannot serialize unresolved alias",
            )),
        }
    }
}

impl<'de> Deserialize<'de> for RegistryRef {
    fn deserialize<D>(deserializer: D) -> Result<RegistryRef, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let url = String::deserialize(deserializer)?;
        RegistryRef::from_str(&url).map_err(serde::de::Error::custom)
    }
}

impl Serialize for RegistryRef {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.to_string().serialize(serializer)
    }
}

impl From<RegistryUri> for Url {
    fn from(value: RegistryUri) -> Self {
        value.0
    }
}

impl TryFrom<RegistryRef> for RegistryUri {
    type Error = miette::Report;

    fn try_from(value: RegistryRef) -> Result<Self, Self::Error> {
        match value {
            RegistryRef::Url(url) => Ok(url),
            RegistryRef::ResolvedAlias { url, .. } => Ok(url),
            _ => Err(miette!(
                "cannot convert unresolved alias \"{value}\" to URL"
            )),
        }
    }
}

impl TryFrom<&RegistryRef> for RegistryUri {
    type Error = miette::Report;

    fn try_from(value: &RegistryRef) -> Result<Self, Self::Error> {
        // Delegate to the implementation for the owned type
        TryFrom::<RegistryRef>::try_from(value.clone())
    }
}

impl Display for RegistryRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RegistryRef::Url(url) => write!(f, "{}", url),
            RegistryRef::Alias(alias) => write!(f, "{}", alias),
            RegistryRef::ResolvedAlias { alias, url } => write!(f, "{} ({})", alias, url),
        }
    }
}

impl FromStr for RegistryRef {
    type Err = miette::Report;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        // Attempt to parse the value as a URL
        match RegistryUri::from_str(value) {
            Ok(uri) => Ok(Self::Url(uri)),
            // If the value is not a valid URL, treat it as an alias
            Err(_) => Ok(Self::Alias(value.parse()?)),
        }
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

/// Ensure that the URL is valid for a registry
///
/// A valid registry URL must:
/// - Have a scheme of either "http" or "https"
/// - End with "/artifactory" if the host is a JFrog Artifactory instance
/// - Have a host component
///
/// # Arguments
/// * `url` - The URL to check
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

    use super::RegistryRef;

    fn get_dependency(version: &str) -> Dependency {
        let registry = RegistryRef::from_str("https://my-registry.com").unwrap();
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
