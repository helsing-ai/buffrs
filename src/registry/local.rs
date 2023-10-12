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
use eyre::{ensure, ContextCompat};
use tokio::fs;

use crate::{manifest::Dependency, package::Package};

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

    pub async fn download(&self, dependency: Dependency) -> eyre::Result<Package> {
        // TODO(rfink): Factor out checks so that artifactory and local registry both use them
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
            "local registry only support pinned dependencies"
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

        let path = self.base_dir.join(PathBuf::from(format!(
            "{}/{}/{}-{}.tgz",
            dependency.manifest.repository, dependency.package, dependency.package, version
        )));

        tracing::debug!("downloaded dependency {dependency} from {:?}", path);

        let bytes = Bytes::from(fs::read(path).await?);
        Package::try_from(bytes)
    }

    pub async fn publish(&self, package: Package, repository: String) -> eyre::Result<()> {
        let path = self.base_dir.join(PathBuf::from(format!(
            "{}/{}/{}-{}.tgz",
            repository,
            package.name(),
            package.name(),
            package.version(),
        )));

        fs::create_dir_all(path.parent().unwrap()).await?;

        fs::write(&path, &package.tgz).await?;

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
    use crate::manifest::{Dependency, Manifest, PackageManifest};
    use crate::package::{Package, PackageType};
    use crate::registry::local::LocalRegistry;
    use bytes::Bytes;
    use std::path::PathBuf;
    use std::{env, fs};

    #[tokio::test]
    async fn can_publish_and_fetch() {
        let dir = env::temp_dir();
        let registry = LocalRegistry::new(dir.clone());

        let manifest = Manifest {
            package: PackageManifest {
                kind: PackageType::Api,
                name: "test-api".parse().unwrap(),
                version: "0.1.0".parse().unwrap(),
                description: None,
            },
            dependencies: vec![],
        };

        let package_bytes =
            Bytes::from(include_bytes!("../../tests/data/packages/test-api-0.1.0.tgz").to_vec());

        // Publish to local registry and assert the tgz exists in the file system
        registry
            .publish(
                Package::new(manifest.clone(), package_bytes.clone()),
                "test-repo".into(),
            )
            .await
            .unwrap();

        assert_eq!(
            Bytes::from(
                fs::read(dir.join(PathBuf::from("test-repo/test-api/test-api-0.1.0.tgz"))).unwrap()
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
