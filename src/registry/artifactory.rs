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
    errors::{HttpError, RequestError, ResponseError},
    manifest::Dependency,
    package::Package,
};
use bytes::Bytes;
use eyre::Context;
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
    pub fn new(registry: RegistryUri, credentials: &Credentials) -> eyre::Result<Self> {
        Ok(Self {
            registry: registry.clone(),
            token: credentials.registry_tokens.get(&registry).cloned(),
            client: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .build()?,
        })
    }

    async fn make_auth_request(
        &self,
        method: Method,
        url: Url,
        body: Option<Bytes>,
    ) -> eyre::Result<Response, HttpError> {
        let mut request_builder = self.client.request(method.clone(), url.clone());

        if let Some(token) = &self.token {
            request_builder = request_builder.bearer_auth(token);
        }

        if let Some(body) = body {
            request_builder = request_builder.body(body);
        }

        let request = request_builder.build().map_err(|err| {
            HttpError::Request(RequestError::create(
                method.clone(),
                url.clone(),
                err,
                Default::default(),
            ))
        })?;

        let headers = request.headers().clone();

        let convert_err = |err: reqwest::Error| {
            let status = err.status();
            let request_ctx =
                RequestError::create(method.clone(), url.clone(), err, headers.clone());
            match status {
                None => HttpError::Request(request_ctx),
                Some(status) => HttpError::Response(ResponseError::new(request_ctx, status)),
            }
        };

        let response: Response = self.client.execute(request).await.map_err(convert_err)?;

        if response.status().is_redirection() {
            return Err(HttpError::Other(format!(
                "remote server attempted to redirect request - is this registry URL valid? {}",
                self.registry
            )));
        }

        if response.status() == 401 {
            return Err(HttpError::Unauthorized);
        }

        response.error_for_status().map_err(convert_err)
    }

    /// Pings artifactory to ensure registry access is working
    pub async fn ping(&self) -> eyre::Result<()> {
        let repositories_url: Url = {
            let mut uri = self.registry.to_owned();
            let path = &format!("{}/api/repositories", uri.path());
            uri.set_path(path);
            uri.into()
        };

        self.make_auth_request(Method::GET, repositories_url, None)
            .await
            .map(|_| ())
            .map_err(eyre::Report::from)
    }

    /// Downloads a package from artifactory
    pub async fn download(&self, dependency: Dependency) -> eyre::Result<Package> {
        if dependency.manifest.version.comparators.len() != 1 {
            eyre::bail!(UnsupportedVersionRequirement(dependency.manifest.version,));
        }

        let version = dependency
            .manifest
            .version
            .comparators
            .first()
            // validated above
            .expect("unexpected error: empty comparators vector in VersionReq");

        if version.op != semver::Op::Exact || version.minor.is_none() || version.patch.is_none() {
            eyre::bail!(UnsupportedVersionRequirement(dependency.manifest.version,));
        }

        let version = format!(
            "{}.{}.{}{}",
            version.major,
            version
                .minor
                .expect("unexpected error: minor number missing"), // validated above
            version
                .patch
                .expect("unexpected error: patch number missing"), // validated above
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
            .ok_or_else(|| HttpError::Other("missing content-type header".into()))?;

        if content_type != reqwest::header::HeaderValue::from_static("application/x-gzip") {
            return Err(HttpError::Other(format!(
                "server response has incorrect mime type: {content_type:?}"
            ))
            .into());
        }

        tracing::debug!("downloaded dependency {dependency}");

        let data = response.bytes().await.map_err(|_| {
            HttpError::Other(format!(
                "failed to download data for dependency {}",
                dependency.package
            ))
        })?;

        Package::try_from(data)
            .wrap_err_with(|| format!("failed to download dependency {}", dependency.package))
    }

    /// Publishes a package to artifactory
    pub async fn publish(&self, package: Package, repository: String) -> eyre::Result<()> {
        let artifact_uri: Url = format!(
            "{}/{}/{}/{}-{}.tgz",
            self.registry,
            repository,
            package.name(),
            package.name(),
            package.version(),
        )
        .parse()
        .expect("unexpected error: failed to construct artifact URL");

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
