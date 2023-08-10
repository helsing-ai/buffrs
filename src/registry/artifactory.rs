// (c) Copyright 2023 Helsing GmbH. All rights reserved.

use std::sync::Arc;

use eyre::{ensure, Context};
use serde::{Deserialize, Serialize};
use url::Url;

use super::Registry;
use crate::{manifest::Dependency, package::Package};

/// The registry implementation for artifactory
#[derive(Debug, Clone)]
pub struct Artifactory(Arc<ArtifactoryConfig>);

impl Artifactory {
    /// Pings artifactory to ensure registry access is working
    pub async fn ping(&self) -> eyre::Result<()> {
        let repositories_uri: Url = {
            let mut url = self.0.url.clone();
            url.set_path(&format!("{}/api/repositories", url.path()));
            url
        };

        let response = reqwest::Client::new()
            .get(repositories_uri.clone())
            .basic_auth(self.0.username.to_owned(), Some(&self.0.password))
            .send()
            .await?;

        ensure!(response.status().is_success(), "Failed to ping artifactory");

        tracing::debug!("pinging artifactory succeeded");

        Ok(())
    }
}

#[async_trait::async_trait]
impl Registry for Artifactory {
    /// Downloads a package from artifactory
    async fn download(&self, dependency: Dependency) -> eyre::Result<Package> {
        let artifact_uri: Url = {
            let mut url = self.0.url.clone();

            url.set_path(&format!(
                "{}/{}/{}/{}-{}.tgz",
                url.path(),
                dependency.manifest.repository,
                dependency.package,
                dependency.package,
                dependency.manifest.version
            ));

            url
        };

        let response = reqwest::Client::new()
            .get(artifact_uri.clone())
            .basic_auth(self.0.username.to_owned(), Some(&self.0.password))
            .send()
            .await?;

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
    async fn publish(&self, package: Package, repository: String) -> eyre::Result<()> {
        let artifact_uri: Url = format!(
            "{}/{}/{}/{}-{}.tgz",
            self.0.url,
            repository,
            package.manifest.name,
            package.manifest.name,
            package.manifest.version,
        )
        .parse()
        .wrap_err("Failed to construct artifact uri")?;

        let response = reqwest::Client::new()
            .put(artifact_uri.clone())
            .basic_auth(self.0.username.to_owned(), Some(&self.0.password))
            .body(package.tgz)
            .send()
            .await
            .wrap_err("Failed to upload release to artifactory")?;

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

impl From<ArtifactoryConfig> for Artifactory {
    fn from(cfg: ArtifactoryConfig) -> Self {
        Self(cfg.into())
    }
}

/// Authentication data and settings for the artifactory registry
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArtifactoryConfig {
    pub url: Url,
    pub username: String,
    pub password: String,
}

impl ArtifactoryConfig {
    /// Creates a new artifactory config in the system keyring
    pub fn new(url: Url, username: String, password: String) -> Self {
        Self {
            url,
            username,
            password,
        }
    }
}
