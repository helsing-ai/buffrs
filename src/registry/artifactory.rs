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

use super::{DownloadError, PublishError, Registry, RegistryUri};
use crate::{credentials::Credentials, manifest::Dependency, package::Package};
use reqwest::Response;
use thiserror::Error;
use url::Url;

/// The registry implementation for artifactory
#[derive(Debug, Clone)]
pub struct Artifactory {
    registry: RegistryUri,
    token: Option<String>,
    client: reqwest::Client,
}

/// Error produced by the ping method
#[derive(Error, Debug)]
#[error(transparent)]
pub struct PingError(#[from] reqwest::Error);

/// Error produced when instantiating an Artifactory registry
#[derive(Error, Debug)]
#[error(transparent)]
pub struct BuildError(#[from] reqwest::Error);

impl Artifactory {
    /// Creates a new instance of an Artifactory registry client
    pub fn new(registry: RegistryUri, credentials: &Credentials) -> Result<Self, BuildError> {
        Ok(Self {
            registry: registry.clone(),
            token: credentials.registry_tokens.get(&registry).cloned(),
            client: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .build()?,
        })
    }

    /// Pings artifactory to ensure registry access is working
    pub async fn ping(&self) -> Result<(), PingError> {
        let repositories_url: Url = {
            let mut uri = self.registry.to_owned();
            let path = &format!("{}/api/repositories", uri.path());
            uri.set_path(path);
            uri.into()
        };

        let mut request = self.client.get(repositories_url);

        if let Some(token) = &self.token {
            request = request.bearer_auth(token);
        }

        let response: Response = request.send().await?;

        let _ = response.error_for_status()?;

        Ok(())
    }

    fn validate_status(&self, response: &Response) -> Result<(), String> {
        if response.status().is_redirection() {
            return Err(format!(
                "remote server attempted to redirect request - is this registry URL valid? {}",
                self.registry
            ));
        }

        if response.status() == 401 {
            return Err(
                "unauthorized - please provide registry credentials with `buffrs login`".into(),
            );
        }

        if !response.status().is_success() {
            return Err(response.status().to_string());
        }

        Ok(())
    }
}

#[async_trait::async_trait]
impl Registry for Artifactory {
    /// Downloads a package from artifactory
    async fn download(&self, dependency: Dependency) -> Result<Package, DownloadError> {
        if dependency.manifest.version.comparators.len() != 1 {
            return Err(DownloadError::UnsupportedVersionRequirement(
                dependency.manifest.version,
            ));
        }

        let version = dependency
            .manifest
            .version
            .comparators
            .first()
            // validated above
            .expect("Unexpected error: empty comparators vector in VersionReq");

        if version.op != semver::Op::Exact || version.minor.is_none() || version.patch.is_none() {
            return Err(DownloadError::UnsupportedVersionRequirement(
                dependency.manifest.version,
            ));
        }

        let version = format!(
            "{}.{}.{}{}",
            version.major,
            version
                .minor
                .expect("Unexpected error: minor number missing"), // validated above
            version
                .patch
                .expect("Unexpected error: patch number missing"), // validated above
            if version.pre.is_empty() {
                "".to_owned()
            } else {
                format!("-{}", version.pre)
            }
        );

        let artifact_url: Url = {
            let path = dependency.manifest.registry.path().to_owned();

            let mut url = dependency.manifest.registry.clone();
            url.set_path(&format!(
                "{}/{}/{}/{}-{}.tgz",
                path,
                dependency.manifest.repository,
                dependency.package,
                dependency.package,
                version
            ));

            url.into()
        };

        tracing::debug!("Hitting download URL: {artifact_url}");

        let mut request = self.client.get(artifact_url);

        if let Some(token) = &self.token {
            request = request.bearer_auth(token);
        }

        let response = request
            .send()
            .await
            .map_err(|err| DownloadError::RequestFailed(err.to_string()))?;

        self.validate_status(&response)
            .map_err(DownloadError::RequestFailed)?;

        let headers = response.headers();
        let content_type =
            headers
                .get(&reqwest::header::CONTENT_TYPE)
                .ok_or(DownloadError::InvalidResponse(
                    "missing content-type header".into(),
                ))?;

        if content_type != reqwest::header::HeaderValue::from_static("application/x-gzip") {
            return Err(DownloadError::InvalidResponse(format!(
                "Server response has incorrect mime type: {content_type:?}"
            )));
        }

        tracing::debug!("downloaded dependency {dependency}");

        let data = response
            .bytes()
            .await
            .map_err(|_| DownloadError::RequestFailed("failed to download data".into()))?;

        Package::try_from(data)
            .map_err(|_| DownloadError::InvalidResponse("failed to decode tarball".into()))
    }

    /// Publishes a package to artifactory
    async fn publish(&self, package: Package, repository: String) -> Result<(), PublishError> {
        let artifact_uri: Url = format!(
            "{}/{}/{}/{}-{}.tgz",
            self.registry,
            repository,
            package.name(),
            package.name(),
            package.version(),
        )
        .parse()
        .map_err(|_| PublishError::RequestFailed("failed to construct artifact uri".into()))?;

        let mut request = self.client.put(artifact_uri).body(package.tgz.clone());

        if let Some(token) = &self.token {
            request = request.bearer_auth(token);
        }

        let response = request
            .send()
            .await
            .map_err(|err| PublishError::RequestFailed(err.to_string()))?;

        self.validate_status(&response)
            .map_err(PublishError::RequestFailed)?;

        tracing::info!(
            ":: published {}/{}@{}",
            repository,
            package.name(),
            package.version()
        );

        Ok(())
    }
}
