use super::*;
use std::collections::HashMap;
use std::fmt::Debug;
use std::sync::Mutex;
use tonic::async_trait;

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

#[async_trait]
impl MetadataStorage for InMemoryMetadataStorage {
    /// fetches the version from the storage
    async fn get_version(
        &self,
        package: PackageVersion,
    ) -> Result<PackageManifest, MetadataStorageError> {
        let packages = self
            .packages
            .lock()
            .map_err(|_| MetadataStorageError::Internal)?;

        let name_string = package.package.to_string();
        let version_string = package.version.to_string();

        let versions_mutex =
            packages
                .get(name_string.as_str())
                .ok_or(MetadataStorageError::PackageMissing(
                    name_string.clone(),
                    version_string.clone(),
                ))?;

        let versions = versions_mutex
            .lock()
            .map_err(|_| MetadataStorageError::Internal)?;

        let package =
            versions
                .get(version_string.as_str())
                .ok_or(MetadataStorageError::PackageMissing(
                    name_string,
                    version_string,
                ))?;

        Ok(package.clone())
    }

    /// Puts a Manifest in the storage
    ///
    async fn put_version(&self, package: PackageManifest) -> Result<(), MetadataStorageError> {
        let mut packages = self
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
