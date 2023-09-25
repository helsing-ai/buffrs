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

use super::{Registry, RegistryUri};
use crate::{credentials::Credentials, manifest::Dependency, package::Package};
use eyre::{ensure, Context, ContextCompat};
use reqwest::header::HeaderMap;
use url::Url;

/// The registry implementation for artifactory
#[derive(Debug, Clone)]
pub struct Artifactory {
    registry: RegistryUri,
    client: reqwest::Client,
}

impl Artifactory {
    const JFROG_AUTH_HEADER: &str = "X-JFrog-Art-Api";

    pub fn new(registry: RegistryUri, credentials: &Credentials) -> eyre::Result<Self> {
        let mut client_builder =
            reqwest::Client::builder().redirect(reqwest::redirect::Policy::none());

        if let Some(token) = credentials.registry_tokens.get(&registry) {
            let mut headers = HeaderMap::new();
            headers.insert(Self::JFROG_AUTH_HEADER, token.parse()?);

            client_builder = client_builder.default_headers(headers);
        }

        Ok(Self {
            registry: registry.clone(),
            client: client_builder.build()?,
        })
    }

    /// Pings artifactory to ensure registry access is working
    pub async fn ping(&self) -> eyre::Result<()> {
        let repositories_url: Url = {
            let mut uri = self.registry.to_owned();
            let path = &format!("{}/api/repositories", uri.path());
            uri.set_path(path);
            uri.into()
        };

        let response = self.client.get(repositories_url).send().await?;

        ensure!(response.status().is_success(), "Failed to ping artifactory");

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

        let response = self.client.get(artifact_url).send().await?;

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

        let response = self
            .client
            .put(artifact_uri)
            .body(package.tgz.clone())
            .send()
            .await
            .wrap_err("Failed to upload release to artifactory")?;

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
