// Copyright 2025 Helsing GmbH
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

use super::{
    RegistryUri,
    http::{RequestBuilder, ValidatedResponse},
};
use crate::{
    credentials::Credentials,
    lock::DigestAlgorithm,
    manifest::{Dependency, DependencyManifest},
    package::{Package, PackageName},
};
use miette::{Context, IntoDiagnostic, ensure, miette};
use reqwest::Method;
use semver::Version;
use serde::Deserialize;
use url::Url;

/// The registry implementation for Maven
#[derive(Debug, Clone)]
pub struct Maven {
    registry: RegistryUri,
    token: Option<String>,
    client: reqwest::Client,
}

impl Maven {
    /// Creates a new instance of a Maven registry client
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
        let request_builder = RequestBuilder::new(self.client.clone(), method, url);

        match &self.token {
            Some(token) => request_builder.auth(token.clone()),
            None => request_builder,
        }
    }

    /// Pings the Maven registry to ensure access is working
    pub async fn ping(&self) -> miette::Result<()> {
        // For Maven, we just try to access the base URL
        let ping_url: Url = self.registry.url().clone();

        self.new_request(Method::GET, ping_url)
            .send()
            .await
            .map(|_| ())
    }

    /// Retrieves the latest version of a package by parsing maven-metadata.xml
    pub async fn get_latest_version(
        &self,
        repository: String,
        name: PackageName,
    ) -> miette::Result<Version> {
        let metadata_url: Url = {
            let mut url = self.registry.url().clone();
            let path = format!(
                "{}/{}/{}/maven-metadata.xml",
                url.path().trim_end_matches('/'),
                repository,
                name
            );
            url.set_path(&path);
            url
        };

        let response = self.new_request(Method::GET, metadata_url).send().await?;
        let response: reqwest::Response = response.0;

        let metadata_xml = response.text().await.into_diagnostic().wrap_err(miette!(
            "unexpected error: unable to retrieve maven-metadata.xml"
        ))?;

        // Parse the XML to extract the latest version
        let metadata: MavenMetadata = quick_xml::de::from_str(&metadata_xml)
            .into_diagnostic()
            .wrap_err(miette!(
                "unexpected error: failed to parse maven-metadata.xml"
            ))?;

        let latest_version_str = metadata
            .versioning
            .latest
            .or(metadata.versioning.release)
            .ok_or_else(|| miette!("no latest version found in maven-metadata.xml"))?;

        Version::parse(&latest_version_str)
            .into_diagnostic()
            .wrap_err(miette!(
                "invalid version in maven-metadata.xml: {}",
                latest_version_str
            ))
    }

    /// Downloads a package from Maven
    pub async fn download(&self, dependency: Dependency) -> miette::Result<Package> {
        let DependencyManifest::Remote(ref manifest) = dependency.manifest else {
            return Err(miette!(
                "unable to download local dependency ({}) from Maven",
                dependency.package
            ));
        };

        let artifact_url = {
            let version = super::dependency_version_string(&dependency)?;

            let path = manifest.registry.url().path().to_owned();

            let mut url = manifest.registry.url().clone();
            url.set_path(&format!(
                "{}/{}/{}/{}/{}-{}.tgz",
                path, manifest.repository, dependency.package, version, dependency.package, version
            ));

            url
        };

        tracing::debug!("Hitting download URL: {artifact_url}");

        let response = self.new_request(Method::GET, artifact_url).send().await?;

        let response: reqwest::Response = response.0;

        let headers = response.headers();
        let content_type = headers
            .get(&reqwest::header::CONTENT_TYPE)
            .ok_or_else(|| miette!("missing content-type header"))?;

        let is_x_gzip =
            content_type == reqwest::header::HeaderValue::from_static("application/x-gzip");
        let is_gzip = content_type == reqwest::header::HeaderValue::from_static("application/gzip");
        let is_octet_stream =
            content_type == reqwest::header::HeaderValue::from_static("application/octet-stream");

        ensure!(
            is_x_gzip || is_gzip || is_octet_stream,
            "server response has incorrect mime type: {content_type:?}"
        );

        let data = response.bytes().await.into_diagnostic()?;

        Package::try_from(data).wrap_err(miette!(
            "failed to download dependency {}",
            dependency.package
        ))
    }

    /// Publishes a package to Maven and updates maven-metadata.xml
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
                "unable to publish {} to Maven due having the following local dependencies: {}",
                package.name(),
                names.join(", ")
            ));
        }

        let version = package.version().to_string();

        // Construct the artifact URL
        let artifact_uri: Url = format!(
            "{}/{}/{}/{}/{}-{}.tgz",
            self.registry,
            repository,
            package.name(),
            version,
            package.name(),
            version,
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
                miette!("unexpected error: failed to read the bytes back from Maven"),
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
                    "unable to publish {} to Maven: package is already published with a different hash",
                    package.name()
                ));
            }
        }

        // Upload the artifact
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

        // Now update maven-metadata.xml
        self.update_maven_metadata(&repository, package.name(), package.version())
            .await?;

        Ok(())
    }

    /// Updates or creates maven-metadata.xml for a package
    async fn update_maven_metadata(
        &self,
        repository: &str,
        package_name: &PackageName,
        new_version: &Version,
    ) -> miette::Result<()> {
        let metadata_url: Url = {
            let mut url = self.registry.url().clone();
            let path = format!(
                "{}/{}/{}/maven-metadata.xml",
                url.path().trim_end_matches('/'),
                repository,
                package_name
            );
            url.set_path(&path);
            url
        };

        // Try to fetch existing metadata
        let existing_metadata = self
            .new_request(Method::GET, metadata_url.clone())
            .send()
            .await;

        let mut metadata = match existing_metadata {
            Ok(ValidatedResponse(response)) if response.status().is_success() => {
                let xml = response.text().await.into_diagnostic()?;
                quick_xml::de::from_str::<MavenMetadata>(&xml)
                    .into_diagnostic()
                    .wrap_err_with(|| miette!("failed to parse existing maven-metadata.xml"))?
            }
            _ => MavenMetadata::new(package_name.to_string()),
        };

        // Add the new version if it doesn't exist
        let version_str = new_version.to_string();
        if !metadata.versioning.versions.contains(&version_str) {
            metadata.versioning.versions.push(version_str.clone());
            // Sort versions
            metadata.versioning.versions.sort_by(|a, b| {
                let va = Version::parse(a).ok();
                let vb = Version::parse(b).ok();
                match (va, vb) {
                    (Some(va), Some(vb)) => va.cmp(&vb),
                    _ => a.cmp(b),
                }
            });
        }

        // Update latest version
        metadata.versioning.latest = Some(version_str.clone());
        metadata.versioning.release = Some(version_str);

        // Update lastUpdated timestamp
        let now = chrono::Utc::now();
        metadata.versioning.last_updated = Some(now.format("%Y%m%d%H%M%S").to_string());

        // Serialize to XML
        let xml = quick_xml::se::to_string(&metadata)
            .into_diagnostic()
            .wrap_err(miette!("failed to serialize maven-metadata.xml"))?;

        // Upload the updated metadata
        self.new_request(Method::PUT, metadata_url)
            .body(xml)
            .send()
            .await?;

        tracing::debug!("Updated maven-metadata.xml for {}", package_name);

        Ok(())
    }
}

/// Maven metadata structure for maven-metadata.xml
#[derive(Debug, Deserialize, serde::Serialize, Clone, PartialEq, Eq)]
struct MavenMetadata {
    #[serde(rename = "groupId", skip_serializing_if = "Option::is_none")]
    group_id: Option<String>,
    #[serde(rename = "artifactId")]
    artifact_id: String,
    versioning: Versioning,
}

impl MavenMetadata {
    fn new(artifact_id: String) -> Self {
        Self {
            group_id: None,
            artifact_id,
            versioning: Versioning {
                latest: None,
                release: None,
                versions: Vec::new(),
                last_updated: None,
            },
        }
    }
}

#[derive(Debug, Deserialize, serde::Serialize, Clone, PartialEq, Eq)]
struct Versioning {
    #[serde(skip_serializing_if = "Option::is_none")]
    latest: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    release: Option<String>,
    #[serde(rename = "versions")]
    versions: Vec<String>,
    #[serde(rename = "lastUpdated", skip_serializing_if = "Option::is_none")]
    last_updated: Option<String>,
}

#[async_trait::async_trait]
impl super::Registry for Maven {
    async fn ping(&self) -> miette::Result<()> {
        self.ping().await
    }

    async fn get_latest_version(
        &self,
        repository: String,
        name: PackageName,
    ) -> miette::Result<Version> {
        self.get_latest_version(repository, name).await
    }

    async fn download(&self, dependency: Dependency) -> miette::Result<Package> {
        self.download(dependency).await
    }

    async fn publish(&self, package: Package, repository: String) -> miette::Result<()> {
        self.publish(package, repository).await
    }
}
