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

use std::collections::HashMap;

use miette::{ensure, Context, IntoDiagnostic};
use semver::Version;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::fs;
use url::Url;

use crate::{
    errors::{DeserializationError, FileExistsError, FileNotFound, SerializationError, WriteError},
    package::{Package, PackageName},
    registry::{RegistryRef, RegistryUri},
    ManagedFile,
};

mod digest;
pub use digest::{Digest, DigestAlgorithm};

/// File name of the lockfile
pub const LOCKFILE: &str = "Proto.lock";

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
    #[serde(serialize_with = "RegistryRef::serialize_resolved")]
    pub registry: RegistryRef,
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
        registry: RegistryRef,
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

#[derive(Serialize, Deserialize)]
struct RawLockfile {
    version: u16,
    packages: Vec<LockedPackage>,
}

/// Captures metadata about currently installed Packages
///
/// Used to ensure future installations will deterministically select the exact same packages.
#[derive(Default)]
pub struct Lockfile {
    packages: HashMap<PackageName, LockedPackage>,
}

impl Lockfile {
    /// Checks if the Lockfile currently exists in the filesystem
    pub async fn exists() -> miette::Result<bool> {
        fs::try_exists(LOCKFILE)
            .await
            .into_diagnostic()
            .wrap_err(FileExistsError(LOCKFILE))
    }

    /// Loads the Lockfile from the current directory
    pub async fn read() -> miette::Result<Self> {
        match fs::read_to_string(LOCKFILE).await {
            Ok(contents) => {
                let raw: RawLockfile = toml::from_str(&contents)
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

    /// Loads the Lockfile from the current directory, if it exists, otherwise returns an empty one
    pub async fn read_or_default() -> miette::Result<Self> {
        if Lockfile::exists().await? {
            Lockfile::read().await
        } else {
            Ok(Lockfile::default())
        }
    }

    /// Persists a Lockfile to the filesystem
    pub async fn write(&self) -> miette::Result<()> {
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

        let raw = RawLockfile {
            version: 1,
            packages,
        };

        fs::write(
            LOCKFILE,
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

impl FromIterator<LockedPackage> for Lockfile {
    fn from_iter<I: IntoIterator<Item = LockedPackage>>(iter: I) -> Self {
        Self {
            packages: iter
                .into_iter()
                .map(|locked| (locked.name.clone(), locked))
                .collect(),
        }
    }
}

impl TryFrom<Lockfile> for Vec<FileRequirement> {
    type Error = miette::Report;

    fn try_from(lock: Lockfile) -> miette::Result<Self> {
        lock.packages
            .values()
            .map(FileRequirement::try_from)
            .collect()
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
        url: &RegistryRef,
        repository: &String,
        name: &PackageName,
        version: &Version,
        digest: &Digest,
    ) -> miette::Result<Self> {
        let url: RegistryUri = url.try_into()?;
        let mut url: url::Url = url.into();

        let new_path = format!(
            "{}/{}/{}/{}-{}.tgz",
            url.path(),
            repository,
            name,
            name,
            version
        );

        url.set_path(&new_path);

        Ok(Self {
            package: name.to_owned(),
            url,
            digest: digest.clone(),
        })
    }
}

impl TryFrom<LockedPackage> for FileRequirement {
    type Error = miette::Report;

    fn try_from(package: LockedPackage) -> miette::Result<Self> {
        Self::new(
            &package.registry,
            &package.repository,
            &package.name,
            &package.version,
            &package.digest,
        )
    }
}

impl TryFrom<&LockedPackage> for FileRequirement {
    type Error = miette::Report;

    fn try_from(package: &LockedPackage) -> miette::Result<Self> {
        Self::new(
            &package.registry,
            &package.repository,
            &package.name,
            &package.version,
            &package.digest,
        )
    }
}
