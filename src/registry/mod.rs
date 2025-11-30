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
mod http;
mod maven;

use artifactory::Artifactory;
use maven::Maven;
use miette::{Context, IntoDiagnostic, ensure, miette};
use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use url::Url;

use crate::{
    manifest::{Dependency, DependencyManifest},
    package::{Package, PackageName},
};

/// Full prefix for Artifactory registry URIs
const ARTIFACTORY_PREFIX: &str = "artifactory+";

/// Full prefix for Maven registry URIs
const MAVEN_PREFIX: &str = "maven+";

/// Trait for registry backends
#[async_trait::async_trait]
pub trait Registry: Send + Sync {
    /// Pings the registry to ensure access is working
    async fn ping(&self) -> miette::Result<()>;

    /// Retrieves the latest version of a package
    async fn get_latest_version(
        &self,
        repository: String,
        name: PackageName,
    ) -> miette::Result<Version>;

    /// Downloads a package from the registry
    async fn download(&self, dependency: Dependency) -> miette::Result<Package>;

    /// Publishes a package to the registry
    async fn publish(&self, package: Package, repository: String) -> miette::Result<()>;
}

/// The type of registry backend
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub enum RegistryType {
    /// Artifactory registry backend
    Artifactory,
    /// Maven registry backend
    Maven,
}

impl RegistryType {
    /// Get the prefix string for this registry type
    fn prefix(&self) -> &'static str {
        match self {
            RegistryType::Artifactory => ARTIFACTORY_PREFIX,
            RegistryType::Maven => MAVEN_PREFIX,
        }
    }

    /// Remove the registry type prefix from a URL string
    fn strip_prefix(s: &str) -> (&str, Option<Self>) {
        if let Some(rest) = s.strip_prefix(ARTIFACTORY_PREFIX) {
            (rest, Some(RegistryType::Artifactory))
        } else if let Some(rest) = s.strip_prefix(MAVEN_PREFIX) {
            (rest, Some(RegistryType::Maven))
        } else {
            (s, None)
        }
    }
}

/// A representation of a registry URI
#[derive(Debug, Clone, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct RegistryUri {
    url: Url,
    registry_type: RegistryType,
}

impl Serialize for RegistryUri {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        // Always serialize with the registry type prefix
        let prefixed = format!("{}{}", self.registry_type.prefix(), self.url);
        serializer.serialize_str(&prefixed)
    }
}

impl<'de> Deserialize<'de> for RegistryUri {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Self::from_str(&s).map_err(serde::de::Error::custom)
    }
}

impl RegistryUri {
    /// Get the registry type for this URI
    pub fn registry_type(&self) -> RegistryType {
        self.registry_type
    }

    /// Get the underlying URL (without the registry type prefix)
    pub fn url(&self) -> &Url {
        &self.url
    }
}

/// Builder for creating registry instances
#[derive(Default)]
pub struct RegistryBuilder<'a> {
    kind: Option<RegistryType>,
    uri: Option<RegistryUri>,
    credentials: Option<&'a crate::credentials::Credentials>,
}

impl<'a> RegistryBuilder<'a> {
    /// Creates a new RegistryBuilder
    pub fn builder() -> Self {
        Self::default()
    }

    /// Sets the registry type/kind
    pub fn kind(mut self, kind: RegistryType) -> Self {
        self.kind = Some(kind);
        self
    }

    /// Sets the registry URI
    pub fn uri(mut self, uri: RegistryUri) -> Self {
        self.uri = Some(uri);
        self
    }

    /// Sets the credentials
    pub fn credentials(mut self, credentials: &'a crate::credentials::Credentials) -> Self {
        self.credentials = Some(credentials);
        self
    }

    /// Builds the registry instance
    pub fn build(self) -> miette::Result<Box<dyn Registry>> {
        let kind = self
            .kind
            .ok_or_else(|| miette!("registry kind is required"))?;
        let uri = self
            .uri
            .ok_or_else(|| miette!("registry URI is required"))?;
        let credentials = self
            .credentials
            .ok_or_else(|| miette!("credentials are required"))?;

        match kind {
            RegistryType::Artifactory => Ok(Box::new(Artifactory::new(uri, credentials)?)),
            RegistryType::Maven => Ok(Box::new(Maven::new(uri, credentials)?)),
        }
    }
}

impl From<RegistryUri> for Url {
    fn from(value: RegistryUri) -> Self {
        value.url
    }
}

impl Display for RegistryUri {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}", self.registry_type.prefix(), self.url)
    }
}

impl FromStr for RegistryUri {
    type Err = miette::Report;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        // Strip the registry type prefix if present, defaulting to Artifactory
        let (url_str, registry_type) = RegistryType::strip_prefix(value);
        let registry_type = registry_type.unwrap_or(RegistryType::Artifactory);

        let url = Url::from_str(url_str)
            .into_diagnostic()
            .wrap_err(miette!("not a valid URL: {value}"))?;

        sanity_check_url(&url)?;

        Ok(Self { url, registry_type })
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
#[error(
    "{0} is not supported yet. Pin the exact version you want to use with '='. For example: '=1.0.4' instead of '^1.0.0'"
)]
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
        registry::{VersionNotPinned, dependency_version_string},
    };

    use super::RegistryUri;

    fn get_dependency(version: &str) -> Dependency {
        let registry = RegistryUri::from_str("https://my-registry.com").unwrap();
        let repository = String::from("my-repo");
        let package = PackageName::from_str("package").unwrap();
        let version = VersionReq::from_str(version).unwrap();
        Dependency::new(registry, repository, package, version)
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
