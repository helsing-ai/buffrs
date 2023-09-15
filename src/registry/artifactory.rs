// (c) Copyright 2023 Helsing GmbH. All rights reserved.

use std::sync::Arc;

use eyre::{ensure, Context, ContextCompat};
use serde::{Deserialize, Serialize};
use url::Url;

use super::Registry;
use crate::{
    manifest::{Dependency, RegistryUrl, Repository},
    package::Package,
};

/// The registry implementation for artifactory
#[derive(Debug, Clone)]
pub struct Artifactory {
    config: Arc<ArtifactoryConfig>,
    registry: RegistryUrl,
}

impl Artifactory {
    pub fn new(config: Arc<ArtifactoryConfig>, registry: RegistryUrl) -> Self {
        Self { config, registry }
    }

    /// Pings artifactory to ensure registry access is working
    pub async fn ping(&self) -> eyre::Result<()> {
        // TODO: kihehs - does this make sense anymore?
        return Ok(());

        // let repositories_uri: Url = {
        //     let mut url = self.0.url.clone();
        //     url.set_path(&format!("{}/api/repositories", url.path()));
        //     url
        // };

        // let response = reqwest::Client::builder()
        //     .redirect(reqwest::redirect::Policy::none())
        //     .build()
        //     .wrap_err("client error")?
        //     .get(repositories_uri.clone())
        //     .header(
        //         "X-JFrog-Art-Api",
        //         self.0.password.clone().unwrap_or_default(),
        //     )
        //     .send()
        //     .await?;

        // ensure!(response.status().is_success(), "Failed to ping artifactory");

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

        let response = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .wrap_err("client error")?
            .get((*artifact_uri).clone())
            .header(
                "X-JFrog-Art-Api",
                self.config.password.clone().unwrap_or_default(),
            )
            .send()
            .await?;

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

        Package::try_from(tgz)
    }

    /// Publishes a package to artifactory
    async fn publish(&self, package: Package, repository: Repository) -> eyre::Result<()> {
        let artifact_uri: Url = format!(
            "{}/{}/{}/{}-{}.tgz",
            self.registry,
            repository,
            package.manifest.name,
            package.manifest.name,
            package.manifest.version,
        )
        .parse()
        .wrap_err("Failed to construct artifact uri")?;

        let response = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .wrap_err("client error")?
            .put(artifact_uri.clone())
            .header(
                "X-JFrog-Art-Api",
                self.config.password.clone().unwrap_or_default(),
            )
            .body(package.tgz)
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
            package.manifest.name,
            response.status()
        );

        tracing::info!(
            ":: published {}/{}@{}",
            repository,
            package.manifest.name,
            package.manifest.version
        );

        Ok(())
    }
}

/// Authentication data and settings for the artifactory registry
#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArtifactoryConfig {
    pub password: Option<String>,
}

impl ArtifactoryConfig {
    /// Creates a new artifactory config in the system keyring
    pub fn new() -> Self {
        Default::default()
    }
}
