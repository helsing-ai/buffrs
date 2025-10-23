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

use std::path::PathBuf;

use bytes::Bytes;
use miette::{Context, IntoDiagnostic, miette};
use tokio::fs;

use crate::{
    manifest::{Dependency, DependencyManifest},
    manifest_v2::PackagesManifest,
    package::Package,
};

/// A registry that stores and retries packages from a local file system.
/// This registry is intended primarily for testing.
#[derive(Debug, Clone)]
pub struct LocalRegistry {
    base_dir: PathBuf,
}

impl LocalRegistry {
    #[allow(dead_code)]
    pub fn new(base_dir: PathBuf) -> Self {
        LocalRegistry { base_dir }
    }

    /// "Downloads" a package from the local filesystem
    pub async fn download(&self, dependency: Dependency) -> miette::Result<Package> {
        let DependencyManifest::Remote(ref manifest) = dependency.manifest else {
            return Err(miette!(
                "unable to serialize version of local dependency ({})",
                dependency.package
            ));
        };

        let version = super::dependency_version_string(&dependency)?;

        let path = self.base_dir.join(PathBuf::from(format!(
            "{}/{}/{}-{}.tgz",
            manifest.repository, dependency.package, dependency.package, version
        )));

        tracing::debug!("downloaded dependency {dependency} from {:?}", path);

        let bytes = Bytes::from(
            fs::read(path)
                .await
                .into_diagnostic()
                .wrap_err(miette!("could not read file"))?,
        );

        Package::try_from(bytes).wrap_err(miette!(
            "failed to download dependency {}",
            dependency.package
        ))
    }

    /// "Publishes" or stores a package in the local store
    pub async fn publish(&self, package: Package, repository: String) -> miette::Result<()> {
        let path = self.base_dir.join(PathBuf::from(format!(
            "{}/{}/{}-{}.tgz",
            repository,
            package.name(),
            package.name(),
            package.version(),
        )));

        fs::create_dir_all(path.parent().unwrap())
            .await
            .into_diagnostic()?;

        fs::write(&path, &package.tgz)
            .await
            .into_diagnostic()
            .wrap_err(miette!("could not write to file: {}", path.display()))?;

        tracing::info!(
            ":: published {}/{}@{} to {:?}",
            repository,
            package.name(),
            package.version(),
            path
        );

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::manifest_v2::PackagesManifest;
    use crate::{
        manifest::{Dependency, Manifest, PackageManifest},
        package::{Package, PackageType},
        registry::cache::LocalRegistry,
    };
    use bytes::Bytes;
    use std::{env, path::PathBuf};
    use tokio::fs;

    #[tokio::test]
    #[ignore = "gzid header issues"]
    async fn can_publish_and_fetch() {
        let dir = env::temp_dir();
        let registry = LocalRegistry::new(dir.clone());

        let manifest = PackagesManifest::builder()
            .package(PackageManifest {
                kind: PackageType::Api,
                name: "test-api".parse().unwrap(),
                version: "0.1.0".parse().unwrap(),
                description: None,
            })
            .dependencies(vec![])
            .build();

        let package_bytes =
            Bytes::from(include_bytes!("../../tests/data/packages/test-api-0.1.0.tgz").to_vec());

        // Publish to local registry and assert the tgz exists in the file system
        registry
            .publish(
                Package {
                    manifest: manifest.clone(),
                    tgz: package_bytes.clone(),
                },
                "test-repo".into(),
            )
            .await
            .unwrap();

        assert_eq!(
            Bytes::from(
                fs::read(dir.join(PathBuf::from("test-repo/test-api/test-api-0.1.0.tgz")))
                    .await
                    .unwrap()
            ),
            package_bytes
        );

        let registry_uri = "http://some-registry/artifactory"
            .parse()
            .expect("Failed to parse registry URL");

        // Download package from local registry and assert the tgz bytes and the metadata match what we
        // had published.
        let fetched = registry
            .download(Dependency::new(
                registry_uri,
                "test-repo".into(),
                "test-api".parse().unwrap(),
                "=0.1.0".parse().unwrap(),
            ))
            .await
            .unwrap();

        assert_eq!(fetched.manifest, manifest);
        assert_eq!(fetched.tgz, package_bytes);
    }
}
