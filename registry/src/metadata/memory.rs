use super::*;

use std::collections::HashMap;
use std::fmt::Debug;
use std::sync::Mutex;

type MemoryMetadataMap = Arc<Mutex<HashMap<String, Mutex<HashMap<String, PackageManifest>>>>>;

/// InMemory provider for MetadataStorage
#[derive(Debug, Default)]
pub struct InMemoryMetadataStorage {
    packages: MemoryMetadataMap,
}

impl InMemoryMetadataStorage {
    /// Creates a new InMemoryMetadataStorage
    pub fn new() -> Self {
        InMemoryMetadataStorage {
            packages: Arc::new(Mutex::new(HashMap::default())),
        }
    }
}

#[async_trait::async_trait]
impl TryFetch<InMemoryMetadataStorage> for PackageManifest {
    async fn try_fetch(
        version: PackageVersion,
        e: &InMemoryMetadataStorage,
    ) -> Result<PackageManifest, MetadataStorageError> {
        let packages = e
            .packages
            .lock()
            .map_err(|_| MetadataStorageError::Internal)?;

        let name_string = version.package.to_string();
        let version_string = version.version.to_string();

        let versions_mutex =
            packages
                .get(name_string.as_str())
                .ok_or(MetadataStorageError::PackageMissing(
                    name_string.clone(),
                    Some(version_string.clone()),
                ))?;

        let versions = versions_mutex
            .lock()
            .map_err(|_| MetadataStorageError::Internal)?;

        let package =
            versions
                .get(version_string.as_str())
                .ok_or(MetadataStorageError::PackageMissing(
                    name_string,
                    Some(version_string),
                ))?;

        Ok(package.clone())
    }
}

#[async_trait::async_trait]
impl FetchAllMatching<InMemoryMetadataStorage> for PackageManifest {
    async fn fetch_matching(
        package: PackageName,
        req: VersionReq,
        e: &InMemoryMetadataStorage,
    ) -> Result<Vec<PackageManifest>, MetadataStorageError> {
        let packages = e
            .packages
            .lock()
            .map_err(|_| MetadataStorageError::Internal)?;

        let package_name = package.to_string();
        let versions_mutex = packages
            .get(package_name.as_str())
            .ok_or(MetadataStorageError::PackageMissing(package_name, None))?;

        let versions = versions_mutex
            .lock()
            .map_err(|_| MetadataStorageError::Internal)?;

        let listed_versions = versions
            .iter()
            .filter(|(_version, manifest)| req.matches(&manifest.version))
            .map(|(_version, manifest)| buffrs::manifest::PackageManifest {
                kind: manifest.kind,
                name: manifest.name.clone(),
                version: manifest.version.clone(),
                description: manifest.description.clone(),
            })
            .collect();

        Ok(listed_versions)
    }
}

#[async_trait::async_trait]
impl Publish<InMemoryMetadataStorage> for PackageManifest {
    async fn publish(
        package: PackageManifest,
        e: &InMemoryMetadataStorage,
    ) -> Result<(), MetadataStorageError> {
        let mut packages = e
            .packages
            .lock()
            .map_err(|_| MetadataStorageError::Internal)?;

        let name_string = package.name.to_string();
        let version_string = package.version.to_string();

        let Some(versions_mutex) = packages.get(name_string.as_str()) else {
            let mut versions_hashmap = HashMap::default();
            let _ = &versions_hashmap.insert(version_string, package);
            let _ = &packages.insert(name_string, Mutex::new(versions_hashmap));

            return Ok(());
        };

        let mut versions = versions_mutex
            .lock()
            .map_err(|_| MetadataStorageError::Internal)?;

        if versions.contains_key(version_string.as_str()) {
            return Err(MetadataStorageError::PackageDuplicate(
                name_string,
                version_string,
            ));
        }

        let _ = &versions.insert(version_string, package);

        Ok(())
    }
}
