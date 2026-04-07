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
use semver::{Version, VersionReq};
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
        tracing::debug!("Artifactory::new() called");
        tracing::debug!("  registry: {}", registry);

        let has_token = credentials.registry_tokens.contains_key(&registry);
        tracing::debug!("  has authentication token: {}", has_token);

        tracing::debug!("creating reqwest client with no redirect policy");
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .into_diagnostic()?;
        tracing::debug!("reqwest client created successfully");

        tracing::debug!("Artifactory client initialized successfully");
        Ok(Self {
            registry: registry.clone(),
            token: credentials.registry_tokens.get(&registry).cloned(),
            client,
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

    /// Lists all available versions of a package from artifactory, sorted descending
    pub async fn list_versions(
        &self,
        repository: String,
        name: PackageName,
    ) -> miette::Result<Vec<Version>> {
        tracing::debug!("Artifactory::list_versions() called");
        tracing::debug!("  package name: {}", name);
        tracing::debug!("  repository: {}", repository);
        tracing::debug!("  registry: {}", self.registry);

        let search_query_url: Url = {
            let mut url = self.registry.clone();
            url.set_path("artifactory/api/search/artifact");
            url.set_query(Some(&format!("name={name}&repos={repository}")));
            url.into()
        };

        tracing::debug!("search query URL: {}", search_query_url);

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
            "found {} artifacts matching the name",
            parsed_response.results.len()
        );

        let mut versions: Vec<Version> = parsed_response
            .results
            .iter()
            .filter_map(|artifact_search_result| {
                let uri = artifact_search_result.to_owned().uri;
                tracing::debug!("  processing artifact URI: {}", uri);

                let full_artifact_name = uri
                    .split('/')
                    .next_back()
                    .map(|name_tgz| name_tgz.trim_end_matches(".tgz"));

                let artifact_version = full_artifact_name
                    .and_then(|name| name.split('-').next_back())
                    .and_then(|version_str| Version::parse(version_str).ok());

                // Double-check that the artifact name matches exactly
                let expected_artifact_name =
                    artifact_version.clone().map(|av| format!("{name}-{av}"));
                if full_artifact_name.is_some_and(|actual| {
                    expected_artifact_name.is_some_and(|expected| expected == actual)
                }) {
                    artifact_version
                } else {
                    tracing::debug!("    artifact name doesn't match expected format, skipping");
                    None
                }
            })
            .collect();

        versions.sort_unstable_by(|a, b| b.cmp(a)); // descending
        tracing::debug!("found {} valid versions", versions.len());
        Ok(versions)
    }

    /// Resolves the highest available version of a package satisfying a requirement
    pub async fn resolve_version(
        &self,
        repository: String,
        name: PackageName,
        req: &VersionReq,
    ) -> miette::Result<Version> {
        let versions = self.list_versions(repository, name.clone()).await?;
        versions
            .into_iter()
            .find(|v| req.matches(v))
            .ok_or_else(|| {
                miette!(
                    "no version of {} satisfies requirement {} in this registry",
                    name,
                    req
                )
            })
    }

    /// Retrieves the latest version of a package by querying artifactory
    pub async fn get_latest_version(
        &self,
        repository: String,
        name: PackageName,
    ) -> miette::Result<Version> {
        tracing::debug!("Artifactory::get_latest_version() called");
        self.list_versions(repository.clone(), name.clone())
            .await?
            .into_iter()
            .next() // list_versions is already sorted descending
            .ok_or_else(|| {
                tracing::error!(
                    "no version could be found for package {} in repository {}",
                    name,
                    repository
                );
                miette!("no version could be found on artifactory for this artifact name. Does it exist in this registry and repository?")
            })
    }

    /// Downloads a specific version of a package from artifactory
    pub async fn download(
        &self,
        dependency: Dependency,
        version: &Version,
    ) -> miette::Result<Package> {
        tracing::debug!("Artifactory::download() called");
        tracing::debug!("  package name: {}", dependency.package);

        let DependencyManifest::Remote(ref manifest) = dependency.manifest else {
            tracing::error!(
                "attempted to download local dependency {} from artifactory",
                dependency.package
            );
            return Err(miette!(
                "unable to download local dependency ({}) from artifactory",
                dependency.package
            ));
        };

        tracing::debug!("  registry: {}", manifest.registry);
        tracing::debug!("  repository: {}", manifest.repository);
        tracing::debug!("  resolved version: {}", version);

        let artifact_url = {
            let path = manifest.registry.path().to_owned();

            let mut url = manifest.registry.clone();
            url.set_path(&format!(
                "{}/{}/{}/{}-{}.tgz",
                path, manifest.repository, dependency.package, dependency.package, version
            ));

            url.into()
        };

        tracing::debug!("constructed download URL: {}", artifact_url);

        tracing::debug!("sending GET request to download package");
        let download_start = std::time::Instant::now();
        let response = self.new_request(Method::GET, artifact_url).send().await?;
        tracing::debug!("received response from artifactory");

        let response: reqwest::Response = response.0;

        let headers = response.headers();
        let content_type = headers
            .get(&reqwest::header::CONTENT_TYPE)
            .ok_or_else(|| miette!("missing content-type header"))?;
        tracing::debug!("response content-type: {:?}", content_type);

        ensure!(
            content_type == reqwest::header::HeaderValue::from_static("application/x-gzip"),
            "server response has incorrect mime type: {content_type:?}"
        );

        tracing::debug!("reading response body as bytes");
        let data = response.bytes().await.into_diagnostic()?;
        let download_duration = download_start.elapsed();
        tracing::debug!("downloaded {} bytes in {:?}", data.len(), download_duration);

        tracing::debug!("parsing package from downloaded data");
        let package = Package::try_from(data).wrap_err(miette!(
            "failed to download dependency {}",
            dependency.package
        ))?;

        tracing::debug!("package {} downloaded successfully", dependency.package);
        Ok(package)
    }

    /// Publishes a package to artifactory
    pub async fn publish(&self, package: Package, repository: String) -> miette::Result<()> {
        tracing::debug!("Artifactory::publish() called");
        tracing::debug!("  package name: {}", package.name());
        tracing::debug!("  package version: {}", package.version());
        tracing::debug!("  repository: {}", repository);
        tracing::debug!("  registry: {}", self.registry);

        let local_deps: Vec<&Dependency> = package
            .manifest
            .dependencies
            .iter()
            .flatten()
            .filter(|d| d.manifest.is_local())
            .collect();

        tracing::debug!("checking for local dependencies in package manifest");
        tracing::debug!(
            "  total dependencies: {}",
            package
                .manifest
                .dependencies
                .as_ref()
                .map(|d| d.len())
                .unwrap_or(0)
        );
        tracing::debug!("  local dependencies found: {}", local_deps.len());

        // abort publishing if we have local dependencies
        if !local_deps.is_empty() {
            let names: Vec<String> = local_deps.iter().map(|d| d.package.to_string()).collect();
            tracing::error!(
                "cannot publish package {} with local dependencies: {}",
                package.name(),
                names.join(", ")
            );

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

        tracing::debug!("constructed artifact URI: {}", artifact_uri);
        tracing::debug!("package tgz size: {} bytes", package.tgz.len());

        // check if the package already exists upstream
        tracing::debug!("checking if package already exists in registry (GET request)");
        let response = self
            .new_request(Method::GET, artifact_uri.clone())
            .send()
            .await;

        // 404 gets wrapped into a DiagnosticError(reqwest::Error(404))
        // so we need to make sure it's OK before unwrapping
        if let Ok(ValidatedResponse(response)) = response {
            let status = response.status();
            tracing::debug!("package exists in registry, status: {}", status);

            if status.is_success() {
                tracing::debug!("package found in registry, comparing hashes");

                // compare hash to make sure the file in the registry is the same
                let alg = DigestAlgorithm::SHA256;
                tracing::debug!("computing SHA256 hash of local package");
                let package_hash = alg.digest(&package.tgz);
                tracing::debug!("  local package hash: {}", package_hash);

                tracing::debug!("fetching and hashing remote package");
                let remote_bytes = response.bytes().await.into_diagnostic().wrap_err(miette!(
                    "unexpected error: failed to read the bytes back from artifactory"
                ))?;
                tracing::debug!("  remote package size: {} bytes", remote_bytes.len());
                let expected_hash = alg.digest(&remote_bytes);
                tracing::debug!("  remote package hash: {}", expected_hash);

                if package_hash == expected_hash {
                    tracing::info!(
                        "{}/{}@{} is already published, skipping",
                        repository,
                        package.name(),
                        package.version()
                    );
                    tracing::debug!("package hashes match, skipping upload");
                    return Ok(());
                } else {
                    tracing::error!(
                        %package_hash,
                        %expected_hash,
                        package = %package.name(),
                        "publishing failed, hash mismatch"
                    );
                    tracing::error!(
                        "local and remote packages have different content but same version"
                    );

                    return Err(miette!(
                        "unable to publish {} to artifactory: package is already published with a different hash",
                        package.name()
                    ));
                }
            }
        } else {
            tracing::debug!(
                "package not found in registry (expected for new packages), proceeding with upload"
            );
        }

        tracing::debug!("uploading package to artifactory (PUT request)");
        tracing::debug!("  upload URI: {}", artifact_uri);
        tracing::debug!("  payload size: {} bytes", package.tgz.len());

        let upload_start = std::time::Instant::now();
        let _ = self
            .new_request(Method::PUT, artifact_uri.clone())
            .body(package.tgz.clone())
            .send()
            .await?;
        let upload_duration = upload_start.elapsed();

        tracing::debug!("upload completed successfully in {:?}", upload_duration);
        tracing::debug!("  uploaded to: {}", artifact_uri);

        tracing::info!(
            "published {}/{}@{}",
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
        tracing::debug!("sending HTTP request");
        let response = self.0.send().await.into_diagnostic()?;
        tracing::debug!("HTTP response received, status: {}", response.status());
        response.try_into()
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
