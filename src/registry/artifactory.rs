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

use std::sync::Arc;

use eyre::{ensure, Context, ContextCompat};
use url::Url;

use super::{Registry, RegistryUri};
use crate::{credentials::Credentials, manifest::Dependency, package::Package};

/// The registry implementation for artifactory
#[derive(Debug, Clone)]
pub struct Artifactory {
    credentials: Arc<Credentials>,
    registry: RegistryUri,
}

impl Artifactory {
    const JFROG_AUTH_HEADER: &str = "X-JFrog-Art-Api";

    pub fn new(credentials: Arc<Credentials>, registry: RegistryUri) -> Self {
        Self {
            credentials,
            registry,
        }
    }

    /// Pings artifactory to ensure registry access is working
    pub async fn ping(&self) -> eyre::Result<()> {
        let repositories_uri = {
            let mut uri = self.registry.to_owned();
            let path = &format!("{}/api/repositories", uri.path());
            uri.set_path(path);
            uri
        };

        let response = {
            let builder = reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .wrap_err("client error")?
                .get(repositories_uri.as_str());

            let builder = if let Some(token) = self.credentials.registry_tokens.get(&self.registry)
            {
                tracing::info!(
                    uri=?repositories_uri.as_str(),
                    length=?token.len(),
                    "Pinging with the {} header set",
                    Self::JFROG_AUTH_HEADER,
                );
                builder.header(Self::JFROG_AUTH_HEADER, token)
            } else {
                tracing::info!(
                    uri=?repositories_uri.as_str(),
                    "Pinging WITHOUT the {} header set",
                    Self::JFROG_AUTH_HEADER
                );
                builder
            };

            builder.send().await?
        };

        let status = response.status();
        ensure!(
            response.status().is_success(),
            "Failed to ping artifactory. Status {status}",
            status = status.as_u16()
        );

        tracing::debug!("pinging artifactory succeeded");

        Ok(())
    }
}

#[async_trait::async_trait]
impl Registry for Artifactory {
    /// Downloads a package from artifactory
    async fn download(&self, dependency: Dependency) -> eyre::Result<Package> {
        ensure!(
            dependency.manifest.version.comparators.len() == 1,
            "{} uses unsupported semver comparators",
            dependency.package
        );

        let version = dependency
            .manifest
            .version
            .comparators
            .first()
            .wrap_err("internal error")?;

        ensure!(
            version.op == semver::Op::Exact && version.minor.is_some() && version.patch.is_some(),
            "artifactory only support pinned dependencies"
        );

        let version = format!(
            "{}.{}.{}{}",
            version.major,
            version.minor.wrap_err("internal error")?,
            version.patch.wrap_err("internal error")?,
            if version.pre.is_empty() {
                "".to_owned()
            } else {
                format!("-{}", version.pre)
            }
        );

        let artifact_uri = {
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

            url
        };

        let response = {
            let builder = reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .wrap_err("client error")?
                .get((*artifact_uri).clone());

            let builder = if let Some(token) = self
                .credentials
                .registry_tokens
                .get(&dependency.manifest.registry)
            {
                builder.header(Self::JFROG_AUTH_HEADER, token)
            } else {
                builder
            };

            builder.send().await?
        };

        ensure!(
            response.status() != 302,
            "Remote server attempted to redirect request - is the Artifactory URL valid?"
        );

        let headers = response.headers();
        let content_type = headers
            .get(&reqwest::header::CONTENT_TYPE)
            .wrap_err("missing header in response")?;

        ensure!(
            content_type == reqwest::header::HeaderValue::from_static("application/x-gzip"),
            "Server response has incorrect mime type: {content_type:?}"
        );

        ensure!(
            response.status().is_success(),
            "Failed to fetch {dependency}: {}",
            response.status()
        );

        tracing::debug!("downloaded dependency {dependency}");

        let tgz = response.bytes().await.wrap_err("Failed to download tar")?;

        Package::try_from(tgz).wrap_err_with(|| {
            format!(
                "Failed to process dependency {}@{}",
                dependency.package, version
            )
        })
    }

    /// Publishes a package to artifactory
    async fn publish(&self, package: Package, repository: String) -> eyre::Result<()> {
        let artifact_uri: Url = format!(
            "{}/{}/{}/{}-{}.tgz",
            self.registry,
            repository,
            package.name(),
            package.name(),
            package.version(),
        )
        .parse()
        .wrap_err("Failed to construct artifact uri")?;

        let response = {
            let builder = reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .wrap_err("client error")?
                .put(artifact_uri.clone())
                .body(package.tgz.clone());

            let builder = if let Some(token) = self.credentials.registry_tokens.get(&self.registry)
            {
                builder.header(Self::JFROG_AUTH_HEADER, token)
            } else {
                builder
            };

            builder
                .send()
                .await
                .wrap_err("Failed to upload release to artifactory")?
        };

        ensure!(
            response.status() != 302,
            "Remote server attempted to redirect publish request - is the Artifactory URL valid?"
        );

        ensure!(
            response.status().is_success(),
            "Failed to publish {}: {}",
            package.name(),
            response.status()
        );

        tracing::info!(
            ":: published {}/{}@{}",
            repository,
            package.name(),
            package.version()
        );

        Ok(())
    }
}
