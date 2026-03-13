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
    errors::{DeserializationError, FileNotFound, SerializationError, WriteError},
    io::File,
    manifest::Manifest,
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
pub enum LockedDependency {
    /// A dependency identified only by name
    Named {
        /// The name of the dependency package
        name: PackageName,
    },
    /// A dependency identified by name and exact version
    Qualified {
        /// The name of the dependency package
        name: PackageName,

        /// The exact version of the dependency package
        version: Version,
    },
}

impl LockedDependency {
    /// Returns the package name of this locked dependency
    pub fn name(&self) -> &PackageName {
        match self {
            Self::Named { name } | Self::Qualified { name, .. } => name,
        }
    }

    /// Returns the version of this locked dependency, if qualified
    pub fn version(&self) -> Option<&Version> {
        match self {
            Self::Qualified { version, .. } => Some(version),
            Self::Named { .. } => None,
        }
    }

    /// Named lock
    pub fn named(name: PackageName) -> Self {
        Self::Named { name }
    }

    /// Qualified lock
    pub fn qualified(name: PackageName, version: Version) -> Self {
        Self::Qualified { name, version }
    }
}

// Custom serialization to match Cargo's "name version" format
impl Serialize for LockedDependency {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            Self::Named { name } => serializer.serialize_str(&format!("{name}")),
            Self::Qualified { name, version } => {
                serializer.serialize_str(&format!("{name} {version}"))
            }
        }
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
            let name = PackageName::new(parts[0])
                .map_err(|e| serde::de::Error::custom(format!("invalid package name: {}", e)))?;

            return Ok(Self::Named { name });
        }

        let name = PackageName::new(parts[0])
            .map_err(|e| serde::de::Error::custom(format!("invalid package name: {}", e)))?;

        let version = Version::parse(parts[1])
            .map_err(|e| serde::de::Error::custom(format!("invalid version: {}", e)))?;

        Ok(LockedDependency::Qualified { name, version })
    }
}

/// Captures immutable metadata about a given package
///
/// It is used to ensure that future installations will use the exact same dependencies.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct LockedPackage {
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
    /// Names of dependency packages
    pub dependencies: Vec<LockedDependency>,
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
                .map(|d| LockedDependency::named(d.package.clone()))
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
#[derive(Default, Debug, PartialEq, Clone)]
pub struct PackageLockfile {
    packages: BTreeMap<PackageName, LockedPackage>,
}

impl PackageLockfile {
    /// Locates a given package in the Lockfile
    pub fn get(&self, name: &PackageName) -> Option<&LockedPackage> {
        self.packages.get(name)
    }

    /// Returns all packages in the lockfile
    pub fn packages(&self) -> impl Iterator<Item = &LockedPackage> {
        self.packages.values()
    }
}

#[async_trait::async_trait]
impl File for PackageLockfile {
    const DEFAULT_PATH: &str = LOCKFILE;

    async fn load_from<P>(path: P) -> miette::Result<Self>
    where
        P: AsRef<Path> + Send + Sync,
    {
        match fs::read_to_string(path).await {
            Ok(contents) => {
                let raw: RawPackageLockfile = toml::from_str(&contents)
                    .into_diagnostic()
                    .wrap_err(DeserializationError(ManagedFile::Lock))?;
                Ok(Self::from_iter(raw.packages))
            }
            Err(err) if matches!(err.kind(), std::io::ErrorKind::NotFound) => {
                Err(FileNotFound(LOCKFILE.into()).into())
            }
            Err(err) => Err(err).into_diagnostic(),
        }
    }

    async fn save<P>(&self, path: P) -> miette::Result<()>
    where
        P: AsRef<Path> + Send + Sync,
    {
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
}

impl From<PackageLockfile> for Vec<FileRequirement> {
    /// Converts lockfile into list of required files
    ///
    /// Must return files with a stable order to ensure identical lockfiles lead to identical
    /// buffrs-cache nix derivations
    fn from(lock: PackageLockfile) -> Self {
        let mut unsorted: Vec<_> = lock.packages.values().collect();

        unsorted.sort_by_key(|c| &c.digest);

        unsorted.into_iter().map(FileRequirement::from).collect()
    }
}

impl TryFrom<Vec<LockedPackage>> for PackageLockfile {
    type Error = miette::Error;

    fn try_from(locked: Vec<LockedPackage>) -> Result<Self, Self::Error> {
        Ok(PackageLockfile::from_iter(locked))
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

#[derive(Serialize, Deserialize)]
struct RawWorkspaceLockfile {
    version: u16,
    packages: Vec<LockedPackage>,
}

impl RawWorkspaceLockfile {
    pub fn v1(packages: Vec<LockedPackage>) -> Self {
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
#[derive(Debug, PartialEq, Clone, Default)]
pub struct WorkspaceLockfile {
    packages: BTreeMap<(PackageName, Version), LockedPackage>,
}

impl WorkspaceLockfile {
    /// Locates a package by name and version
    pub fn get(&self, name: &PackageName, version: &Version) -> Option<&LockedPackage> {
        self.packages.get(&(name.clone(), version.clone()))
    }

    /// Returns all packages in the lockfile
    pub fn packages(&self) -> impl Iterator<Item = &LockedPackage> {
        self.packages.values()
    }
}

#[async_trait::async_trait]
impl File for WorkspaceLockfile {
    const DEFAULT_PATH: &str = LOCKFILE;

    /// Loads the workspace lockfile from a specific path
    async fn load_from<P>(path: P) -> miette::Result<Self>
    where
        P: AsRef<Path> + Send + Sync,
    {
        let path = path.as_ref();

        let resolved = if !path.is_file() {
            path.join(Self::DEFAULT_PATH)
        } else {
            path.to_path_buf()
        };

        match fs::read_to_string(resolved).await {
            Ok(contents) => {
                let raw: RawWorkspaceLockfile = toml::from_str(&contents)
                    .into_diagnostic()
                    .wrap_err(DeserializationError(ManagedFile::Lock))?;
                Ok(Self::from_iter(raw.packages))
            }
            Err(err) if matches!(err.kind(), std::io::ErrorKind::NotFound) => {
                Err(FileNotFound(LOCKFILE.into()).into())
            }
            Err(err) => Err(err).into_diagnostic(),
        }
    }

    /// Persists the workspace lockfile to the filesystem
    async fn save<P>(&self, path: P) -> miette::Result<()>
    where
        P: AsRef<Path> + Send + Sync,
    {
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
}

/// This converts the results of package install to a workspace lockfile
impl FromIterator<LockedPackage> for WorkspaceLockfile {
    fn from_iter<I: IntoIterator<Item = LockedPackage>>(iter: I) -> Self {
        Self {
            packages: iter
                .into_iter()
                .map(|locked| ((locked.name.clone(), locked.version.clone()), locked))
                .collect(),
        }
    }
}

impl From<WorkspaceLockfile> for Vec<FileRequirement> {
    /// Converts lockfile into list of required files
    ///
    /// Must return files with a stable order to ensure identical lockfiles lead to identical
    /// buffrs-cache nix derivations
    fn from(lock: WorkspaceLockfile) -> Self {
        let mut unsorted: Vec<_> = lock.packages.values().collect();

        unsorted.sort_by_key(|c| &c.digest);

        unsorted.into_iter().map(FileRequirement::from).collect()
    }
}

/// A unified view over either a package or workspace lockfile
#[derive(Debug, Clone)]
pub enum Lockfile {
    /// A single-package lockfile
    Package(PackageLockfile),
    /// A workspace-level lockfile
    Workspace(WorkspaceLockfile),
}

impl Lockfile {
    /// Locates a package by name and version
    pub fn get(&self, name: &PackageName, version: &Version) -> Option<FileRequirement> {
        match self {
            Self::Package(lock) => lock
                .get(name)
                .filter(|p| p.version == *version)
                .map(FileRequirement::from),
            Self::Workspace(lock) => lock.get(name, version).map(FileRequirement::from),
        }
    }

    /// Returns all packages in the lockfile
    pub fn packages(&self) -> impl Iterator<Item = &LockedPackage> {
        let pkgs: Vec<&LockedPackage> = match self {
            Self::Package(pkg) => pkg.packages().collect(),
            Self::Workspace(wrk) => wrk.packages().collect(),
        };

        pkgs.into_iter()
    }

    /// Loads an existing lockfile or creates a default one, using the manifest to determine
    /// whether this is a package or workspace lockfile.
    ///
    /// The manifest is always consulted because the package and workspace lockfile formats
    /// are structurally identical in TOML and cannot be distinguished by content alone.
    pub async fn load_from_or_infer(path: impl AsRef<Path>) -> miette::Result<Self> {
        let path = path.as_ref();

        let cwd = if path.is_dir() {
            path
        } else {
            path.parent().ok_or(miette::miette!(
                "Current working directory does not have a basename"
            ))?
        };

        let manifest = Manifest::load_from(cwd).await.wrap_err(miette::miette!(
            "Failed to infer lockfile format, no manifest found in cwd {}",
            cwd.display()
        ))?;

        let lock = if manifest.to_package_manifest().is_ok() {
            PackageLockfile::load_from_or_default(path)
                .await
                .map(Self::Package)?
        } else {
            WorkspaceLockfile::load_from_or_default(path)
                .await
                .map(Self::Workspace)?
        };

        lock.save(path).await?;

        Ok(lock)
    }

    /// Returns true if this is a package lockfile
    pub fn is_package_lockfile(&self) -> bool {
        match self {
            Self::Package(_) => true,
            Self::Workspace(_) => false,
        }
    }

    /// Returns true if this is a workspace lockfile
    pub fn is_workspace_lockfile(&self) -> bool {
        match self {
            Self::Package(_) => false,
            Self::Workspace(_) => true,
        }
    }

    /// Converts into a package lockfile, returning an error if this is a workspace lockfile
    pub fn into_package_lockfile(self) -> miette::Result<PackageLockfile> {
        match self {
            Self::Package(p) => Ok(p),
            Self::Workspace(_) => Err(miette::miette!(
                "A package lockfile was expected but a workspace lockfile was found"
            )),
        }
    }

    /// Converts into a workspace lockfile, returning an error if this is a package lockfile
    pub fn into_workspace_lockfile(self) -> miette::Result<WorkspaceLockfile> {
        match self {
            Self::Workspace(w) => Ok(w),
            Self::Package(_) => Err(miette::miette!(
                "A workspace lockfile was expected but a package lockfile was found"
            )),
        }
    }
}

#[async_trait::async_trait]
impl File for Lockfile {
    const DEFAULT_PATH: &str = LOCKFILE;

    /// Loads the Lockfile from a specific path.
    async fn load_from<P>(path: P) -> miette::Result<Self>
    where
        P: AsRef<Path> + Send + Sync,
    {
        let path = path.as_ref();

        let path = if !path.is_file() {
            path.join(Self::DEFAULT_PATH)
        } else {
            path.to_path_buf()
        };

        let plock = PackageLockfile::load_from(&path).await.map(Self::Package);
        let wlock = WorkspaceLockfile::load_from(&path)
            .await
            .map(Self::Workspace);

        wlock.or(plock)
    }

    /// Persists a Lockfile to the filesystem
    async fn save<P>(&self, path: P) -> miette::Result<()>
    where
        P: AsRef<Path> + Send + Sync,
    {
        match self {
            Self::Package(plock) => plock.save(path).await,
            Self::Workspace(wlock) => wlock.save(path).await,
        }
    }
}

impl From<Lockfile> for Vec<FileRequirement> {
    fn from(value: Lockfile) -> Self {
        match value {
            Lockfile::Package(plock) => plock.into(),
            Lockfile::Workspace(wlock) => wlock.into(),
        }
    }
}

/// Aggregates locked packages from multiple workspace members into a workspace lockfile
///
/// Merges packages by (name, version), deduplicating and summing dependants counts.
impl TryFrom<Vec<LockedPackage>> for WorkspaceLockfile {
    type Error = miette::Report;

    fn try_from(locked_packages: Vec<LockedPackage>) -> Result<Self, Self::Error> {
        use std::collections::BTreeMap;

        let mut workspace_packages: BTreeMap<(PackageName, semver::Version), LockedPackage> =
            BTreeMap::new();

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

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, str::FromStr};

    use semver::Version;

    use crate::{io::File, package::PackageName, registry::RegistryUri};

    use super::{
        Digest, DigestAlgorithm, FileRequirement, LockedDependency, LockedPackage, PackageLockfile,
        WorkspaceLockfile,
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

        let lockfile = PackageLockfile::load_from_or_default(&lockfile_path)
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
        original_lockfile.save(temp_dir.path()).await.unwrap();

        // Read it back using read_from_or_default
        let loaded_lockfile = PackageLockfile::load_from_or_default(&lockfile_path)
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
            LockedDependency::qualified(
                PackageName::unchecked("remote-lib-a"),
                Version::new(1, 5, 0),
            ),
            LockedDependency::qualified(
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
            *deserialized.dependencies[0].name(),
            PackageName::unchecked("remote-lib-a")
        );
        assert_eq!(
            deserialized.dependencies[0].version().cloned(),
            Some(Version::new(1, 5, 0))
        );
        assert_eq!(
            *deserialized.dependencies[1].name(),
            PackageName::unchecked("remote-lib-b")
        );
        assert_eq!(
            deserialized.dependencies[1].version().cloned(),
            Some(Version::new(2, 0, 1))
        );
    }

    #[test]
    fn test_workspace_lockfile_serialization() {
        // Create a workspace lockfile with two packages, one with dependencies
        let pkg1 = LockedPackage {
            name: PackageName::unchecked("remote-lib-a"),
            version: Version::new(1, 0, 0),
            registry: RegistryUri::from_str("https://my-registry.com").unwrap(),
            repository: "test-repo".to_string(),
            digest: Digest::from_parts(
                DigestAlgorithm::SHA256,
                "c109c6b120c525e6ea7b2db98335d39a3272f572ac86ba7b2d65c765c353c122",
            )
            .unwrap(),
            dependencies: vec![LockedDependency::qualified(
                PackageName::unchecked("remote-lib-b"),
                Version::new(1, 5, 0),
            )],
            dependants: 2,
        };

        let pkg2 = LockedPackage {
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
        let pkg_v1 = LockedPackage {
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

        let pkg_v2 = LockedPackage {
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

    #[test]
    fn test_lockfile_package_returns_file_requirement() {
        let lockfile = simple_lockfile();
        let resolved = super::Lockfile::Package(lockfile);

        // Should find package1 at version 0.1.0
        let result = resolved.get(
            &PackageName::new("package1").unwrap(),
            &Version::new(0, 1, 0),
        );
        assert!(result.is_some());
        let file_req = result.unwrap();
        assert!(file_req.url().as_str().contains("package1"));

        // Should return None for wrong version
        let result = resolved.get(
            &PackageName::new("package1").unwrap(),
            &Version::new(9, 9, 9),
        );
        assert!(result.is_none());

        // Should return None for unknown package
        let result = resolved.get(
            &PackageName::new("unknown").unwrap(),
            &Version::new(0, 1, 0),
        );
        assert!(result.is_none());
    }

    #[test]
    fn test_lockfile_workspace_returns_file_requirement() {
        let pkg = LockedPackage {
            name: PackageName::unchecked("ws-pkg"),
            version: Version::new(1, 0, 0),
            registry: RegistryUri::from_str("https://registry.example.com").unwrap(),
            repository: "repo".to_string(),
            digest: Digest::from_parts(
                DigestAlgorithm::SHA256,
                "c109c6b120c525e6ea7b2db98335d39a3272f572ac86ba7b2d65c765c353c122",
            )
            .unwrap(),
            dependencies: vec![],
            dependants: 1,
        };
        let lockfile = WorkspaceLockfile::from_iter(vec![pkg]);
        let resolved = super::Lockfile::Workspace(lockfile);

        // Should find ws-pkg at version 1.0.0
        let result = resolved.get(&PackageName::unchecked("ws-pkg"), &Version::new(1, 0, 0));
        assert!(result.is_some());

        // Should return None for wrong version
        let result = resolved.get(&PackageName::unchecked("ws-pkg"), &Version::new(2, 0, 0));
        assert!(result.is_none());
    }
}
