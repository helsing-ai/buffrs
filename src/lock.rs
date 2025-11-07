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

use std::{collections::BTreeMap, path::Path};

use miette::{Context, IntoDiagnostic, ensure};
use semver::Version;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::fs;
use url::Url;

use crate::{
    ManagedFile,
    errors::{DeserializationError, FileExistsError, FileNotFound, SerializationError, WriteError},
    package::{Package, PackageName},
    registry::RegistryUri,
};

mod digest;
pub use digest::{Digest, DigestAlgorithm};

/// File name of the lockfile
pub const LOCKFILE: &str = "Proto.lock";

/// A locked dependency with exact name and version
///
/// Serializes as "name version" string (Cargo format)
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct LockedDependency {
    /// The name of the dependency package
    pub name: PackageName,
    /// The exact version of the dependency package
    pub version: Version,
}

impl LockedDependency {
    /// Creates a new LockedDependency
    pub fn new(name: PackageName, version: Version) -> Self {
        Self { name, version }
    }
}

// Custom serialization to match Cargo's "name version" format
impl Serialize for LockedDependency {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&format!("{} {}", self.name, self.version))
    }
}

// Custom deserialization from "name version" format
impl<'de> Deserialize<'de> for LockedDependency {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let parts: Vec<&str> = s.split_whitespace().collect();

        if parts.len() != 2 {
            return Err(serde::de::Error::custom(format!(
                "invalid locked dependency format: expected 'name version', got '{}'",
                s
            )));
        }

        let name = PackageName::new(parts[0])
            .map_err(|e| serde::de::Error::custom(format!("invalid package name: {}", e)))?;
        let version = Version::parse(parts[1])
            .map_err(|e| serde::de::Error::custom(format!("invalid version: {}", e)))?;

        Ok(LockedDependency { name, version })
    }
}

/// Captures immutable metadata about a given package
///
/// It is used to ensure that future installations will use the exact same dependencies.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct LockedPackage {
    /// The name of the package
    pub name: PackageName,
    /// The cryptographic digest of the package contents
    pub digest: Digest,
    /// The URI of the registry that contains the package
    pub registry: RegistryUri,
    /// The identifier of the repository where the package was published
    pub repository: String,
    /// The exact version of the package
    pub version: Version,
    /// Names of dependency packages
    pub dependencies: Vec<PackageName>,
    /// Count of dependant packages in the current graph
    ///
    /// This is used to detect when an entry can be safely removed from the lockfile.
    pub dependants: usize,
}

impl LockedPackage {
    /// Captures the source, version and checksum of a Package for use in reproducible installs
    pub fn lock(
        package: &Package,
        registry: RegistryUri,
        repository: String,
        dependants: usize,
    ) -> Self {
        Self {
            name: package.name().to_owned(),
            registry,
            repository,
            digest: package.digest(DigestAlgorithm::SHA256).to_owned(),
            version: package.version().to_owned(),
            dependencies: package
                .manifest
                .dependencies
                .iter()
                .flatten()
                .map(|d| d.package.clone())
                .collect(),
            dependants,
        }
    }

    /// Validates if another LockedPackage matches this one
    pub fn validate(&self, package: &Package) -> miette::Result<()> {
        let digest: Digest = DigestAlgorithm::SHA256.digest(&package.tgz);

        #[derive(Error, Debug)]
        #[error("{property} mismatch - expected {expected}, actual {actual}")]
        struct ValidationError {
            property: &'static str,
            expected: String,
            actual: String,
        }

        ensure!(
            &self.name == package.name(),
            ValidationError {
                property: "name",
                expected: self.name.to_string(),
                actual: package.name().to_string(),
            }
        );

        ensure!(
            &self.version == package.version(),
            ValidationError {
                property: "version",
                expected: self.version.to_string(),
                actual: package.version().to_string(),
            }
        );

        ensure!(
            self.digest == digest,
            ValidationError {
                property: "digest",
                expected: self.digest.to_string(),
                actual: digest.to_string(),
            }
        );

        Ok(())
    }
}

impl From<&WorkspaceLockedPackage> for LockedPackage {
    fn from(ws_locked: &WorkspaceLockedPackage) -> Self {
        Self {
            name: ws_locked.name.clone(),
            version: ws_locked.version.clone(),
            digest: ws_locked.digest.clone(),
            registry: ws_locked.registry.clone(),
            repository: ws_locked.repository.clone(),
            dependencies: ws_locked
                .dependencies
                .iter()
                .map(|d| d.name.clone())
                .collect(),
            dependants: ws_locked.dependants,
        }
    }
}

#[derive(Serialize, Deserialize)]
struct RawPackageLockfile {
    version: u16,
    packages: Vec<LockedPackage>,
}

impl RawPackageLockfile {
    pub fn v1(packages: Vec<LockedPackage>) -> Self {
        Self {
            version: 1,
            packages,
        }
    }
}

/// Captures metadata about currently installed Packages
///
/// Used to ensure future installations will deterministically select the exact same packages.
#[derive(Default, Debug, PartialEq)]
pub struct PackageLockfile {
    packages: BTreeMap<PackageName, LockedPackage>,
}

impl PackageLockfile {
    /// Checks if the Lockfile currently exists in the filesystem
    pub async fn exists() -> miette::Result<bool> {
        Self::exists_at(LOCKFILE).await
    }

    /// Checks if the Lockfile currently exists in the filesystem at a given path
    pub async fn exists_at(path: impl AsRef<Path>) -> miette::Result<bool> {
        fs::try_exists(path)
            .await
            .into_diagnostic()
            .wrap_err(FileExistsError(LOCKFILE))
    }

    /// Loads the Lockfile from the current directory
    pub async fn read() -> miette::Result<Self> {
        Self::read_from(LOCKFILE).await
    }

    /// Loads the Lockfile from a specific path.
    pub async fn read_from(path: impl AsRef<Path>) -> miette::Result<Self> {
        match fs::read_to_string(path).await {
            Ok(contents) => {
                let raw: RawPackageLockfile = toml::from_str(&contents)
                    .into_diagnostic()
                    .wrap_err(DeserializationError(ManagedFile::Lock))?;
                Ok(Self::from_iter(raw.packages.into_iter()))
            }
            Err(err) if matches!(err.kind(), std::io::ErrorKind::NotFound) => {
                Err(FileNotFound(LOCKFILE.into()).into())
            }
            Err(err) => Err(err).into_diagnostic(),
        }
    }

    /// Loads the Lockfile from the current directory, if it exists, otherwise returns an empty one. Fails, if the exists() check fails
    pub async fn read_or_default() -> miette::Result<Self> {
        if PackageLockfile::exists().await? {
            PackageLockfile::read().await
        } else {
            Ok(PackageLockfile::default())
        }
    }

    /// Loads the Lockfile from a specific path, if it exists, otherwise returns an empty one. Fails, if the exists() check fails
    pub async fn read_from_or_default(path: impl AsRef<Path>) -> miette::Result<Self> {
        if PackageLockfile::exists_at(&path).await? {
            PackageLockfile::read_from(path).await
        } else {
            Ok(PackageLockfile::default())
        }
    }

    /// Persists a Lockfile to the filesystem
    pub async fn write(&self, path: impl AsRef<Path>) -> miette::Result<()> {
        let mut packages: Vec<_> = self
            .packages
            .values()
            .map(|pkg| {
                let mut locked = pkg.clone();
                locked.dependencies.sort();
                locked
            })
            .collect();

        packages.sort();

        let raw = RawPackageLockfile::v1(packages);
        let lockfile_path = path.as_ref().join(LOCKFILE);

        fs::write(
            lockfile_path,
            toml::to_string(&raw)
                .into_diagnostic()
                .wrap_err(SerializationError(ManagedFile::Lock))?
                .into_bytes(),
        )
        .await
        .into_diagnostic()
        .wrap_err(WriteError(LOCKFILE))
    }

    /// Locates a given package in the Lockfile
    pub fn get(&self, name: &PackageName) -> Option<&LockedPackage> {
        self.packages.get(name)
    }
}

impl TryFrom<Vec<WorkspaceLockedPackage>> for PackageLockfile {
    type Error = miette::Error;

    fn try_from(locked: Vec<WorkspaceLockedPackage>) -> Result<Self, Self::Error> {
        let package_locked: Vec<LockedPackage> =
            locked.iter().map(LockedPackage::from).collect();

        Ok(PackageLockfile::from_iter(package_locked))
    }
}

impl FromIterator<LockedPackage> for PackageLockfile {
    fn from_iter<I: IntoIterator<Item = LockedPackage>>(iter: I) -> Self {
        Self {
            packages: iter
                .into_iter()
                .map(|locked| (locked.name.clone(), locked))
                .collect(),
        }
    }
}

/// Captures immutable metadata about a package in a workspace lockfile
///
/// Similar to LockedPackage, but includes versioned dependencies to support
/// multiple versions of the same package in a workspace.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct WorkspaceLockedPackage {
    /// The name of the package
    pub name: PackageName,
    /// The exact version of the package
    pub version: Version,
    /// The cryptographic digest of the package contents
    pub digest: Digest,
    /// The URI of the registry that contains the package
    pub registry: RegistryUri,
    /// The identifier of the repository where the package was published
    pub repository: String,
    /// Locked dependencies with exact versions
    #[serde(default)]
    pub dependencies: Vec<LockedDependency>,
    /// Count of dependant packages in the workspace
    pub dependants: usize,
}

impl WorkspaceLockedPackage {
    /// Creates a WorkspaceLockedPackage from a LockedPackage
    pub fn from_locked_package(locked: LockedPackage, dependencies: Vec<LockedDependency>) -> Self {
        Self {
            name: locked.name,
            version: locked.version,
            digest: locked.digest,
            registry: locked.registry,
            repository: locked.repository,
            dependencies,
            dependants: locked.dependants,
        }
    }

    /// Validates if another WorkspaceLockedPackage matches this one
    pub fn validate(&self, package: &Package) -> miette::Result<()> {
        let digest: Digest = DigestAlgorithm::SHA256.digest(&package.tgz);

        #[derive(Error, Debug)]
        #[error("{property} mismatch - expected {expected}, actual {actual}")]
        struct ValidationError {
            property: &'static str,
            expected: String,
            actual: String,
        }

        ensure!(
            &self.name == package.name(),
            ValidationError {
                property: "name",
                expected: self.name.to_string(),
                actual: package.name().to_string(),
            }
        );

        ensure!(
            &self.version == package.version(),
            ValidationError {
                property: "version",
                expected: self.version.to_string(),
                actual: package.version().to_string(),
            }
        );

        ensure!(
            self.digest == digest,
            ValidationError {
                property: "digest",
                expected: self.digest.to_string(),
                actual: digest.to_string(),
            }
        );

        Ok(())
    }
}

#[derive(Serialize, Deserialize)]
struct RawWorkspaceLockfile {
    version: u16,
    packages: Vec<WorkspaceLockedPackage>,
}

impl RawWorkspaceLockfile {
    pub fn v1(packages: Vec<WorkspaceLockedPackage>) -> Self {
        Self {
            version: 1,
            packages,
        }
    }
}

/// Captures metadata about packages installed in a workspace
///
/// Unlike package lockfiles which can only store one version per package,
/// workspace lockfiles use (name, version) as the key to support multiple
/// versions of the same package.
#[derive(Debug, PartialEq)]
pub struct WorkspaceLockfile {
    packages: BTreeMap<(PackageName, Version), WorkspaceLockedPackage>,
}

impl WorkspaceLockfile {
    /// Checks if the workspace lockfile exists at the given path
    pub async fn exists_at(path: impl AsRef<Path>) -> miette::Result<bool> {
        fs::try_exists(path)
            .await
            .into_diagnostic()
            .wrap_err(FileExistsError(LOCKFILE))
    }

    /// Loads the workspace lockfile from a specific path
    pub async fn read_from(path: impl AsRef<Path>) -> miette::Result<Self> {
        match fs::read_to_string(path).await {
            Ok(contents) => {
                let raw: RawWorkspaceLockfile = toml::from_str(&contents)
                    .into_diagnostic()
                    .wrap_err(DeserializationError(ManagedFile::Lock))?;
                Ok(Self::from_iter(raw.packages.into_iter()))
            }
            Err(err) if matches!(err.kind(), std::io::ErrorKind::NotFound) => {
                Err(FileNotFound(LOCKFILE.into()).into())
            }
            Err(err) => Err(err).into_diagnostic(),
        }
    }

    /// Persists the workspace lockfile to the filesystem
    pub async fn write(&self, path: impl AsRef<Path>) -> miette::Result<()> {
        let mut packages: Vec<_> = self
            .packages
            .values()
            .map(|pkg| {
                let mut locked = pkg.clone();
                locked.dependencies.sort();
                locked
            })
            .collect();

        packages.sort();

        let raw = RawWorkspaceLockfile::v1(packages);
        let lockfile_path = path.as_ref().join(LOCKFILE);

        fs::write(
            lockfile_path,
            toml::to_string(&raw)
                .into_diagnostic()
                .wrap_err(SerializationError(ManagedFile::Lock))?
                .into_bytes(),
        )
        .await
        .into_diagnostic()
        .wrap_err(WriteError(LOCKFILE))
    }

    /// Locates a package by name and version
    pub fn get(&self, name: &PackageName, version: &Version) -> Option<&WorkspaceLockedPackage> {
        self.packages.get(&(name.clone(), version.clone()))
    }

    /// Returns all packages in the lockfile
    pub fn packages(&self) -> impl Iterator<Item = &WorkspaceLockedPackage> {
        self.packages.values()
    }
}

/// This converts the results of package install to a workspace lockfile
impl FromIterator<WorkspaceLockedPackage> for WorkspaceLockfile {
    fn from_iter<I: IntoIterator<Item = WorkspaceLockedPackage>>(iter: I) -> Self {
        Self {
            packages: iter
                .into_iter()
                .map(|locked| ((locked.name.clone(), locked.version.clone()), locked))
                .collect(),
        }
    }
}

/// Aggregates locked packages from multiple workspace members into a workspace lockfile
///
/// Merges packages by (name, version), deduplicating and summing dependants counts.
impl TryFrom<Vec<WorkspaceLockedPackage>> for WorkspaceLockfile {
    type Error = miette::Report;

    fn try_from(locked_packages: Vec<WorkspaceLockedPackage>) -> Result<Self, Self::Error> {
        use std::collections::BTreeMap;

        let mut workspace_packages: BTreeMap<
            (PackageName, semver::Version),
            WorkspaceLockedPackage,
        > = BTreeMap::new();

        for locked in locked_packages {
            let key = (locked.name.clone(), locked.version.clone());

            workspace_packages
                .entry(key)
                .and_modify(|existing| {
                    // Same package (name, version) - sum dependants
                    existing.dependants += locked.dependants;

                    // Verify consistency of other fields
                    if existing.registry != locked.registry {
                        tracing::warn!(
                            "registry mismatch for {}@{}: {} vs {}. Using first seen.",
                            locked.name,
                            locked.version,
                            existing.registry,
                            locked.registry
                        );
                    }
                    if existing.digest != locked.digest {
                        tracing::warn!(
                            "digest mismatch for {}@{}: {} vs {}. Using first seen.",
                            locked.name,
                            locked.version,
                            existing.digest,
                            locked.digest
                        );
                    }
                    // Dependencies should be identical for same (name, version)
                    if existing.dependencies != locked.dependencies {
                        tracing::warn!(
                            "dependencies mismatch for {}@{}: {:?} vs {:?}. Using first seen.",
                            locked.name,
                            locked.version,
                            existing.dependencies,
                            locked.dependencies
                        );
                    }
                })
                .or_insert(locked);
        }

        Ok(Self::from_iter(workspace_packages.into_values()))
    }
}

impl From<PackageLockfile> for Vec<FileRequirement> {
    /// Converts lockfile into list of required files
    ///
    /// Must return files with a stable order to ensure identical lockfiles lead to identical
    /// buffrs-cache nix derivations
    fn from(lock: PackageLockfile) -> Self {
        lock.packages.values().map(FileRequirement::from).collect()
    }
}

/// A requirement from a lockfile on a specific file being available in order to build the
/// overall graph. It's expected that when a file is downloaded, it's made available to buffrs
/// by setting the filename to the digest in whatever download directory.
#[derive(Serialize, Clone, PartialEq, Eq)]
pub struct FileRequirement {
    pub(crate) package: PackageName,
    pub(crate) url: Url,
    pub(crate) digest: Digest,
}

impl FileRequirement {
    /// URL where the file can be located.
    pub fn url(&self) -> &Url {
        &self.url
    }

    /// Construct new file requirement.
    pub fn new(
        url: &RegistryUri,
        repository: &String,
        name: &PackageName,
        version: &Version,
        digest: &Digest,
    ) -> Self {
        let mut url = url.clone();
        let new_path = format!(
            "{}/{}/{}/{}-{}.tgz",
            url.path(),
            repository,
            name,
            name,
            version
        );

        url.set_path(&new_path);

        Self {
            package: name.to_owned(),
            url: url.into(),
            digest: digest.clone(),
        }
    }
}

impl From<LockedPackage> for FileRequirement {
    fn from(package: LockedPackage) -> Self {
        Self::new(
            &package.registry,
            &package.repository,
            &package.name,
            &package.version,
            &package.digest,
        )
    }
}

impl From<&LockedPackage> for FileRequirement {
    fn from(package: &LockedPackage) -> Self {
        Self::new(
            &package.registry,
            &package.repository,
            &package.name,
            &package.version,
            &package.digest,
        )
    }
}

impl From<WorkspaceLockedPackage> for FileRequirement {
    fn from(package: WorkspaceLockedPackage) -> Self {
        Self::new(
            &package.registry,
            &package.repository,
            &package.name,
            &package.version,
            &package.digest,
        )
    }
}

impl From<&WorkspaceLockedPackage> for FileRequirement {
    fn from(package: &WorkspaceLockedPackage) -> Self {
        Self::new(
            &package.registry,
            &package.repository,
            &package.name,
            &package.version,
            &package.digest,
        )
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, str::FromStr};

    use semver::Version;

    use crate::{package::PackageName, registry::RegistryUri};

    use super::{
        Digest, DigestAlgorithm, FileRequirement, LockedDependency, LockedPackage, PackageLockfile,
        WorkspaceLockedPackage, WorkspaceLockfile,
    };

    fn simple_lockfile() -> PackageLockfile {
        PackageLockfile {
            packages: BTreeMap::from([
                (
                    PackageName::new("package1").unwrap(),
                    LockedPackage {
                        name: PackageName::new("package1").unwrap(),
                        digest: Digest::from_parts(
                            DigestAlgorithm::SHA256,
                            "c109c6b120c525e6ea7b2db98335d39a3272f572ac86ba7b2d65c765c353c122",
                        )
                        .unwrap(),
                        registry: RegistryUri::from_str("http://my-registry.com").unwrap(),
                        repository: "my-repo".to_owned(),
                        version: Version::new(0, 1, 0),
                        dependencies: Default::default(),
                        dependants: 1,
                    },
                ),
                (
                    PackageName::new("package2").unwrap(),
                    LockedPackage {
                        name: PackageName::new("package2").unwrap(),
                        digest: Digest::from_parts(
                            DigestAlgorithm::SHA256,
                            "c109c6b120c525e6ea7b2db98335d39a3272f572ac86ba7b2d65c765c353bce3",
                        )
                        .unwrap(),
                        registry: RegistryUri::from_str("http://my-registry.com").unwrap(),
                        repository: "my-other-repo".to_owned(),
                        version: Version::new(0, 2, 0),
                        dependencies: Default::default(),
                        dependants: 1,
                    },
                ),
                (
                    PackageName::new("package3").unwrap(),
                    LockedPackage {
                        name: PackageName::new("package3").unwrap(),
                        digest: Digest::from_parts(
                            DigestAlgorithm::SHA256,
                            "c109c6b120c525e6ea7b2db98335d39a3272f572ac86ba7b2d65c765c353bce3",
                        )
                        .unwrap(),
                        registry: RegistryUri::from_str("http://your-registry.com").unwrap(),
                        repository: "your-repo".to_owned(),
                        version: Version::new(0, 2, 0),
                        dependencies: Default::default(),
                        dependants: 1,
                    },
                ),
                (
                    PackageName::new("package4").unwrap(),
                    LockedPackage {
                        name: PackageName::new("package4").unwrap(),
                        digest: Digest::from_parts(
                            DigestAlgorithm::SHA256,
                            "c109c6b120c525e6ea7b2db98335d39a3272f572ac86ba7b2d65c765c353bce3",
                        )
                        .unwrap(),
                        registry: RegistryUri::from_str("http://your-registry.com").unwrap(),
                        repository: "your-other-repo".to_owned(),
                        version: Version::new(0, 2, 0),
                        dependencies: Default::default(),
                        dependants: 1,
                    },
                ),
            ]),
        }
    }

    #[test]
    fn stable_file_requirement_order() {
        let lock = simple_lockfile();
        let files: Vec<FileRequirement> = lock.into();
        for _ in 0..30 {
            let other_files: Vec<FileRequirement> = simple_lockfile().into();
            assert!(other_files == files)
        }
    }

    #[tokio::test]
    async fn test_exists_at_returns_false_for_nonexistent_file() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let lockfile_path = temp_dir.path().join("Proto.lock");

        let exists = PackageLockfile::exists_at(&lockfile_path).await.unwrap();
        assert!(!exists);
    }

    #[tokio::test]
    async fn test_exists_at_returns_true_for_existing_file() {
        use tempfile::TempDir;
        use tokio::fs;

        let temp_dir = TempDir::new().unwrap();
        let lockfile_path = temp_dir.path().join("Proto.lock");

        // Create an empty lockfile
        fs::write(&lockfile_path, "").await.unwrap();

        let exists = PackageLockfile::exists_at(&lockfile_path).await.unwrap();
        assert!(exists);
    }

    #[tokio::test]
    async fn test_exists_at_accepts_reference_and_owned() {
        use std::path::PathBuf;
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let lockfile_path = temp_dir.path().join("Proto.lock");

        // Test with reference
        let exists_ref = PackageLockfile::exists_at(&lockfile_path).await.unwrap();
        assert!(!exists_ref);

        // Test with owned PathBuf
        let lockfile_path_owned = PathBuf::from(&lockfile_path);
        let exists_owned = PackageLockfile::exists_at(lockfile_path_owned)
            .await
            .unwrap();
        assert!(!exists_owned);

        // Test with &str
        let path_str = lockfile_path.to_str().unwrap();
        let exists_str = PackageLockfile::exists_at(path_str).await.unwrap();
        assert!(!exists_str);
    }

    #[tokio::test]
    async fn test_read_from_or_default_returns_default_when_file_missing() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let lockfile_path = temp_dir.path().join("Proto.lock");

        let lockfile = PackageLockfile::read_from_or_default(&lockfile_path)
            .await
            .unwrap();

        assert_eq!(lockfile.packages.len(), 0);
        assert_eq!(lockfile, PackageLockfile::default());
    }

    #[tokio::test]
    async fn test_read_from_or_default_reads_existing_file() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let lockfile_path = temp_dir.path().join("Proto.lock");

        // Create and write a lockfile (write expects directory path)
        let original_lockfile = simple_lockfile();
        original_lockfile.write(temp_dir.path()).await.unwrap();

        // Read it back using read_from_or_default
        let loaded_lockfile = PackageLockfile::read_from_or_default(&lockfile_path)
            .await
            .unwrap();

        assert_eq!(loaded_lockfile.packages.len(), 4);
        assert!(
            loaded_lockfile
                .packages
                .contains_key(&PackageName::new("package1").unwrap())
        );
        assert!(
            loaded_lockfile
                .packages
                .contains_key(&PackageName::new("package2").unwrap())
        );
        assert!(
            loaded_lockfile
                .packages
                .contains_key(&PackageName::new("package3").unwrap())
        );
        assert!(
            loaded_lockfile
                .packages
                .contains_key(&PackageName::new("package4").unwrap())
        );
    }

    #[test]
    fn test_locked_dependency_serialization() {
        // Test serialization within a vector (how it's actually used)
        let deps = vec![
            LockedDependency::new(
                PackageName::unchecked("remote-lib-a"),
                Version::new(1, 5, 0),
            ),
            LockedDependency::new(
                PackageName::unchecked("remote-lib-b"),
                Version::new(2, 0, 1),
            ),
        ];

        #[derive(serde::Serialize, serde::Deserialize)]
        struct TestWrapper {
            dependencies: Vec<LockedDependency>,
        }

        let wrapper = TestWrapper { dependencies: deps };
        let serialized = toml::to_string(&wrapper).unwrap();

        // Verify Cargo-style "name version" format
        assert!(serialized.contains("dependencies = ["));
        assert!(serialized.contains("\"remote-lib-a 1.5.0\""));
        assert!(serialized.contains("\"remote-lib-b 2.0.1\""));

        // Verify round-trip
        let deserialized: TestWrapper = toml::from_str(&serialized).unwrap();
        assert_eq!(deserialized.dependencies.len(), 2);
        assert_eq!(
            deserialized.dependencies[0].name,
            PackageName::unchecked("remote-lib-a")
        );
        assert_eq!(deserialized.dependencies[0].version, Version::new(1, 5, 0));
        assert_eq!(
            deserialized.dependencies[1].name,
            PackageName::unchecked("remote-lib-b")
        );
        assert_eq!(deserialized.dependencies[1].version, Version::new(2, 0, 1));
    }

    #[test]
    fn test_workspace_lockfile_serialization() {
        // Create a workspace lockfile with two packages, one with dependencies
        let pkg1 = WorkspaceLockedPackage {
            name: PackageName::unchecked("remote-lib-a"),
            version: Version::new(1, 0, 0),
            registry: RegistryUri::from_str("https://my-registry.com").unwrap(),
            repository: "test-repo".to_string(),
            digest: Digest::from_parts(
                DigestAlgorithm::SHA256,
                "c109c6b120c525e6ea7b2db98335d39a3272f572ac86ba7b2d65c765c353c122",
            )
            .unwrap(),
            dependencies: vec![LockedDependency::new(
                PackageName::unchecked("remote-lib-b"),
                Version::new(1, 5, 0),
            )],
            dependants: 2,
        };

        let pkg2 = WorkspaceLockedPackage {
            name: PackageName::unchecked("remote-lib-b"),
            version: Version::new(1, 5, 0),
            registry: RegistryUri::from_str("https://my-registry.com").unwrap(),
            repository: "test-repo".to_string(),
            digest: Digest::from_parts(
                DigestAlgorithm::SHA256,
                "c109c6b120c525e6ea7b2db98335d39a3272f572ac86ba7b2d65c765c353bce3",
            )
            .unwrap(),
            dependencies: vec![], // Leaf package
            dependants: 1,
        };

        let lockfile = WorkspaceLockfile::from_iter(vec![pkg1, pkg2]);

        // Serialize to TOML
        let serialized = toml::to_string(&super::RawWorkspaceLockfile {
            version: 1,
            packages: lockfile.packages.values().cloned().collect(),
        })
        .unwrap();

        // Verify format matches expected structure
        assert!(serialized.contains("version = 1"));
        assert!(serialized.contains("[[packages]]"));
        assert!(serialized.contains("name = \"remote-lib-a\""));
        assert!(serialized.contains("version = \"1.0.0\""));
        assert!(serialized.contains("dependencies = [\"remote-lib-b 1.5.0\"]"));
        assert!(serialized.contains("dependants = 2"));
        assert!(serialized.contains("name = \"remote-lib-b\""));
        assert!(serialized.contains("version = \"1.5.0\""));
        assert!(serialized.contains("dependencies = []"));
        assert!(serialized.contains("dependants = 1"));

        // Verify round-trip deserialization
        let raw: super::RawWorkspaceLockfile = toml::from_str(&serialized).unwrap();
        assert_eq!(raw.version, 1);
        assert_eq!(raw.packages.len(), 2);

        let restored = WorkspaceLockfile::from_iter(raw.packages);
        assert_eq!(restored.packages.len(), 2);

        // Verify we can look up by (name, version)
        let found = restored.get(
            &PackageName::unchecked("remote-lib-a"),
            &Version::new(1, 0, 0),
        );
        assert!(found.is_some());
        assert_eq!(found.unwrap().dependencies.len(), 1);
    }

    #[test]
    fn test_workspace_lockfile_supports_multiple_versions() {
        // Create two versions of the same package
        let pkg_v1 = WorkspaceLockedPackage {
            name: PackageName::unchecked("remote-lib"),
            version: Version::new(1, 0, 0),
            registry: RegistryUri::from_str("https://my-registry.com").unwrap(),
            repository: "test-repo".to_string(),
            digest: Digest::from_parts(
                DigestAlgorithm::SHA256,
                "c109c6b120c525e6ea7b2db98335d39a3272f572ac86ba7b2d65c765c353c122",
            )
            .unwrap(),
            dependencies: vec![],
            dependants: 1,
        };

        let pkg_v2 = WorkspaceLockedPackage {
            name: PackageName::unchecked("remote-lib"),
            version: Version::new(2, 0, 0),
            registry: RegistryUri::from_str("https://my-registry.com").unwrap(),
            repository: "test-repo".to_string(),
            digest: Digest::from_parts(
                DigestAlgorithm::SHA256,
                "c109c6b120c525e6ea7b2db98335d39a3272f572ac86ba7b2d65c765c353bce3",
            )
            .unwrap(),
            dependencies: vec![],
            dependants: 1,
        };

        let lockfile = WorkspaceLockfile::from_iter(vec![pkg_v1, pkg_v2]);

        // Both versions should be stored
        assert_eq!(lockfile.packages.len(), 2);

        // Should be able to look up each version independently
        let v1 = lockfile.get(
            &PackageName::unchecked("remote-lib"),
            &Version::new(1, 0, 0),
        );
        assert!(v1.is_some());
        assert_eq!(v1.unwrap().version, Version::new(1, 0, 0));

        let v2 = lockfile.get(
            &PackageName::unchecked("remote-lib"),
            &Version::new(2, 0, 0),
        );
        assert!(v2.is_some());
        assert_eq!(v2.unwrap().version, Version::new(2, 0, 0));
    }
}
