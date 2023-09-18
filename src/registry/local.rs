// (c) Copyright 2023 Helsing GmbH. All rights reserved.

use std::path::PathBuf;

use bytes::Bytes;
use eyre::{ensure, ContextCompat};

use crate::{
    manifest::{Dependency, Repository},
    package::Package,
};

use super::Registry;

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
}

#[async_trait::async_trait]
impl Registry for LocalRegistry {
    async fn download(&self, dependency: Dependency) -> eyre::Result<Package> {
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

        let bytes = Bytes::from(std::fs::read(path)?);
        Package::try_from(bytes)
    }

    async fn publish(&self, package: Package, repository: Repository) -> eyre::Result<()> {
        let path = self.base_dir.join(PathBuf::from(format!(
            "{}/{}/{}-{}.tgz",
            repository, package.manifest.name, package.manifest.name, package.manifest.version,
        )));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();

        std::fs::write(&path, package.tgz)?;
        tracing::info!(
            ":: published {}/{}@{} to {:?}",
            repository,
            package.manifest.name,
            package.manifest.version,
            path
        );

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::manifest::{Dependency, PackageManifest, RegistryUri};
    use crate::package::{Package, PackageId, PackageType};
    use crate::registry::local::LocalRegistry;
    use crate::registry::Registry;
    use bytes::Bytes;
    use semver::{Version, VersionReq};
    use std::path::PathBuf;
    use std::str::FromStr;
    use std::{env, fs};

    #[tokio::test]
    async fn can_publish_and_fetch() {
        let dir = env::temp_dir();
        let registry = LocalRegistry::new(dir.clone());

        let manifest = PackageManifest {
            r#type: PackageType::Api,
            name: PackageId::from_str("test-api").unwrap(),
            version: Version {
                major: 0,
                minor: 1,
                patch: 0,
                pre: Default::default(),
                build: Default::default(),
            },
            description: None,
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

        let registry_uri = RegistryUri::from_str("http://some-registry/artifactory")
            .expect("Failed to parse registry URL");

        // Download package from local registry and assert the tgz bytes and the metadata match what we
        // had published.
        let fetched = registry
            .download(Dependency::new(
                registry_uri,
                "test-repo".into(),
                PackageId::from_str("test-api").unwrap(),
                VersionReq::from_str("=0.1.0").unwrap(),
            ))
            .await
            .unwrap();

        assert_eq!(fetched.manifest, manifest);
        assert_eq!(fetched.tgz, package_bytes);
    }
}
