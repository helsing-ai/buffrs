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
use crate::{credentials::Credentials, manifest::Dependency, package::Package};
use miette::{ensure, miette, Context, IntoDiagnostic};
use reqwest::{Body, Method, Response};
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
            .map_err(miette::Report::from)
    }

    /// Downloads a package from artifactory
    pub async fn download(&self, dependency: Dependency) -> miette::Result<Package> {
        let artifact_url = {
            let version = super::dependency_version_string(&dependency)?;

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

        let response = self.new_request(Method::GET, artifact_url).send().await?;

        let response: reqwest::Response = response.into();

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

struct ValidatedResponse(reqwest::Response);

impl From<ValidatedResponse> for reqwest::Response {
    fn from(value: ValidatedResponse) -> Self {
        value.0
    }
}

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
