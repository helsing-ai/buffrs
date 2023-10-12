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
use bytes::Bytes;
use miette::{ensure, miette, Context, IntoDiagnostic};
use reqwest::{Method, Response};
use semver::VersionReq;
use thiserror::Error;
use url::Url;

/// The registry implementation for artifactory
#[derive(Debug, Clone)]
pub struct Artifactory {
    registry: RegistryUri,
    token: Option<String>,
    client: reqwest::Client,
}

#[derive(Error, Debug)]
#[error("{0} is not a supported version requirement")]
struct UnsupportedVersionRequirement(VersionReq);

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

    async fn make_auth_request(
        &self,
        method: Method,
        url: Url,
        body: Option<Bytes>,
    ) -> miette::Result<Response> {
        let mut request_builder = self.client.request(method, url);

        if let Some(token) = &self.token {
            request_builder = request_builder.bearer_auth(token);
        }

        if let Some(body) = body {
            request_builder = request_builder.body(body);
        }

        let response = request_builder.send().await.into_diagnostic()?;

        ensure!(
            !response.status().is_redirection(),
            "remote server attempted to redirect request - is this registry URL valid? {}",
            self.registry
        );

        ensure!(
            response.status() != 401,
            "unauthorized - please provide registry credentials with `buffrs login`"
        );

        response.error_for_status().into_diagnostic()
    }

    /// Pings artifactory to ensure registry access is working
    pub async fn ping(&self) -> miette::Result<()> {
        let repositories_url: Url = {
            let mut uri = self.registry.to_owned();
            let path = &format!("{}/api/repositories", uri.path());
            uri.set_path(path);
            uri.into()
        };

        self.make_auth_request(Method::GET, repositories_url, None)
            .await
            .map(|_| ())
            .map_err(miette::Report::from)
    }

    /// Downloads a package from artifactory
    pub async fn download(&self, dependency: Dependency) -> miette::Result<Package> {
        let version = dependency
            .manifest
            .version
            .comparators
            .first()
            .ok_or_else(|| UnsupportedVersionRequirement(dependency.manifest.version.clone()))
            .into_diagnostic()?;

        ensure!(
            version.op == semver::Op::Exact,
            UnsupportedVersionRequirement(dependency.manifest.version,)
        );

        let minor_version = version
            .minor
            .ok_or_else(|| miette!("version missing minor number"))?;

        let patch_version = version
            .patch
            .ok_or_else(|| miette!("version missing patch number"))?;

        let version = format!(
            "{}.{}.{}{}",
            version.major,
            minor_version,
            patch_version,
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

        let response = self
            .make_auth_request(Method::GET, artifact_url, None)
            .await?;

        let headers = response.headers();
        let content_type = headers
            .get(&reqwest::header::CONTENT_TYPE)
            .ok_or_else(|| miette!("missing content-type header"))?;

        ensure!(
            content_type == reqwest::header::HeaderValue::from_static("application/x-gzip"),
            "server response has incorrect mime type: {content_type:?}"
        );

        tracing::debug!("downloaded dependency {dependency}");

        let data = response.bytes().await.into_diagnostic().wrap_err_with(|| {
            format!(
                "failed to download data for dependency {}",
                dependency.package
            )
        })?;

        Package::try_from(data)
            .wrap_err_with(|| format!("failed to download dependency {}", dependency.package))
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
            .make_auth_request(Method::PUT, artifact_uri, Some(package.tgz.clone()))
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
