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

use super::RegistryUri;
use crate::{
    credentials::Credentials,
    lock::DigestAlgorithm,
    manifest::{Dependency, DependencyManifest},
    package::{Package, PackageName},
};
use miette::{Context, IntoDiagnostic, ensure, miette};
use reqwest::{Body, Method, Response};
use semver::Version;
use serde::Deserialize;
use url::Url;

/// The registry implementation for artifactory
#[derive(Debug, Clone)]
pub struct Artifactory {
    registry: RegistryUri,
    token: Option<String>,
    client: reqwest::Client,
}

impl Artifactory {
    /// Creates a new instance of an Artifactory registry client
    pub fn new(registry: RegistryUri, credentials: &Credentials) -> miette::Result<Self> {
        Ok(Self {
            registry: registry.clone(),
            token: credentials.registry_tokens.get(&registry).cloned(),
            client: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .into_diagnostic()?,
        })
    }

    fn new_request(&self, method: Method, url: Url) -> RequestBuilder {
        let mut request_builder = RequestBuilder::new(self.client.clone(), method, url);

        if let Some(token) = &self.token {
            request_builder = request_builder.auth(token.clone());
        }

        request_builder
    }

    /// Pings artifactory to ensure registry access is working
    pub async fn ping(&self) -> miette::Result<()> {
        let repositories_url: Url = {
            let mut uri = self.registry.to_owned();
            let path = &format!("{}/api/repositories", uri.path());
            uri.set_path(path);
            uri.into()
        };

        self.new_request(Method::GET, repositories_url)
            .send()
            .await
            .map(|_| ())
    }

    /// Retrieves the latest version of a package by querying artifactory. Returns an error if no artifact could be found
    pub async fn get_latest_version(
        &self,
        repository: String,
        name: PackageName,
    ) -> miette::Result<Version> {
        // First retrieve all packages matching the given name
        let search_query_url: Url = {
            let mut url = self.registry.clone();
            url.set_path("artifactory/api/search/artifact");
            url.set_query(Some(&format!("name={name}&repos={repository}")));
            url.into()
        };

        let response = self
            .new_request(Method::GET, search_query_url)
            .send()
            .await?;
        let response: reqwest::Response = response.0;

        let headers = response.headers();
        let content_type = headers
            .get(&reqwest::header::CONTENT_TYPE)
            .ok_or_else(|| miette!("missing content-type header"))?;
        ensure!(
            content_type
                == reqwest::header::HeaderValue::from_static(
                    "application/vnd.org.jfrog.artifactory.search.ArtifactSearchResult+json"
                ),
            "server response has incorrect mime type: {content_type:?}"
        );

        let response_str = response.text().await.into_diagnostic().wrap_err(miette!(
            "unexpected error: unable to retrieve response payload"
        ))?;
        let parsed_response = serde_json::from_str::<ArtifactSearchResponse>(&response_str)
            .into_diagnostic()
            .wrap_err(miette!(
                "unexpected error: response could not be deserialized to ArtifactSearchResponse"
            ))?;

        tracing::debug!(
            "List of artifacts found matching the name: {:?}",
            parsed_response
        );

        // Then from all package names retrieved from artifactory, extract the highest version number
        let highest_version = parsed_response
            .results
            .iter()
            .filter_map(|artifact_search_result| {
                let uri = artifact_search_result.to_owned().uri;
                let full_artifact_name = uri
                    .split('/')
                    .next_back()
                    .map(|name_tgz| name_tgz.trim_end_matches(".tgz"));
                let artifact_version = full_artifact_name
                    .and_then(|name| name.split('-').next_back())
                    .and_then(|version_str| Version::parse(version_str).ok());

                // we double check that the artifact name matches exactly
                let expected_artifact_name =
                    artifact_version.clone().map(|av| format!("{name}-{av}"));
                if full_artifact_name.is_some_and(|actual| {
                    expected_artifact_name.is_some_and(|expected| expected == actual)
                }) {
                    artifact_version
                } else {
                    None
                }
            })
            .max();

        tracing::debug!("Highest version for artifact: {:?}", highest_version);
        highest_version.ok_or_else(|| {
            miette!("no version could be found on artifactory for this artifact name. Does it exist in this registry and repository?")
        })
    }

    /// Downloads a package from artifactory
    pub async fn download(&self, dependency: Dependency) -> miette::Result<Package> {
        let DependencyManifest::Remote(ref manifest) = dependency.manifest else {
            return Err(miette!(
                "unable to download local dependency ({}) from artifactory",
                dependency.package
            ));
        };

        let artifact_url = {
            let version = super::dependency_version_string(&dependency)?;

            let path = manifest.registry.path().to_owned();

            let mut url = manifest.registry.clone();
            url.set_path(&format!(
                "{}/{}/{}/{}-{}.tgz",
                path, manifest.repository, dependency.package, dependency.package, version
            ));

            url.into()
        };

        tracing::debug!("Hitting download URL: {artifact_url}");

        let response = self.new_request(Method::GET, artifact_url).send().await?;

        let response: reqwest::Response = response.0;

        let headers = response.headers();
        let content_type = headers
            .get(&reqwest::header::CONTENT_TYPE)
            .ok_or_else(|| miette!("missing content-type header"))?;

        ensure!(
            content_type == reqwest::header::HeaderValue::from_static("application/x-gzip"),
            "server response has incorrect mime type: {content_type:?}"
        );

        let data = response.bytes().await.into_diagnostic()?;

        Package::try_from(data).wrap_err(miette!(
            "failed to download dependency {}",
            dependency.package
        ))
    }

    /// Publishes a package to artifactory
    pub async fn publish(&self, package: Package, repository: String) -> miette::Result<()> {
        let local_deps: Vec<&Dependency> = package
            .manifest
            .dependencies
            .iter()
            .flatten()
            .filter(|d| d.manifest.is_local())
            .collect();

        // abort publishing if we have local dependencies
        if !local_deps.is_empty() {
            let names: Vec<String> = local_deps.iter().map(|d| d.package.to_string()).collect();

            return Err(miette!(
                "unable to publish {} to artifactory due having the following local dependencies: {}",
                package.name(),
                names.join(", ")
            ));
        }

        let artifact_uri: Url = format!(
            "{}/{}/{}/{}-{}.tgz",
            self.registry,
            repository,
            package.name(),
            package.name(),
            package.version(),
        )
        .parse()
        .into_diagnostic()
        .wrap_err(miette!(
            "unexpected error: failed to construct artifact URL"
        ))?;

        // check if the package already exists upstream
        let response = self
            .new_request(Method::GET, artifact_uri.clone())
            .send()
            .await;

        // 404 gets wrapped into a DiagnosticError(reqwest::Error(404))
        // so we need to make sure it's OK before unwrapping
        if let Ok(ValidatedResponse(response)) = response
            && response.status().is_success()
        {
            // compare hash to make sure the file in the registry is the same
            let alg = DigestAlgorithm::SHA256;
            let package_hash = alg.digest(&package.tgz);
            let expected_hash = alg.digest(&response.bytes().await.into_diagnostic().wrap_err(
                miette!("unexpected error: failed to read the bytes back from artifactory"),
            )?);
            if package_hash == expected_hash {
                tracing::info!(
                    ":: {}/{}@{} is already published, skipping",
                    repository,
                    package.name(),
                    package.version()
                );
                return Ok(());
            } else {
                tracing::error!(
                    %package_hash,
                    %expected_hash,
                    package = %package.name(),
                    "publishing failed, hash mismatch"
                );
                return Err(miette!(
                    "unable to publish {} to artifactory: package is already published with a different hash",
                    package.name()
                ));
            }
        }

        let _ = self
            .new_request(Method::PUT, artifact_uri)
            .body(package.tgz.clone())
            .send()
            .await?;

        tracing::info!(
            ":: published {}/{}@{}",
            repository,
            package.name(),
            package.version()
        );

        Ok(())
    }
}

struct RequestBuilder(reqwest::RequestBuilder);

impl RequestBuilder {
    fn new(client: reqwest::Client, method: reqwest::Method, url: Url) -> Self {
        Self(client.request(method, url))
    }

    fn auth(mut self, token: String) -> Self {
        self.0 = self.0.bearer_auth(token);
        self
    }

    fn body(mut self, payload: impl Into<Body>) -> Self {
        self.0 = self.0.body(payload);
        self
    }

    async fn send(self) -> miette::Result<ValidatedResponse> {
        self.0.send().await.into_diagnostic()?.try_into()
    }
}

#[derive(Debug)]
struct ValidatedResponse(reqwest::Response);

impl TryFrom<Response> for ValidatedResponse {
    type Error = miette::Report;

    fn try_from(value: Response) -> Result<Self, Self::Error> {
        ensure!(
            !value.status().is_redirection(),
            "remote server attempted to redirect request - is this registry URL valid?"
        );

        ensure!(
            value.status() != 401,
            "unauthorized - please provide registry credentials with `buffrs login`"
        );

        value.error_for_status().into_diagnostic().map(Self)
    }
}

#[derive(Debug, Deserialize, Clone, PartialEq, Eq)]
struct ArtifactSearchResponse {
    results: Vec<ArtifactSearchResult>,
}

#[derive(Debug, Deserialize, Clone, PartialEq, Eq)]
struct ArtifactSearchResult {
    uri: String,
}
