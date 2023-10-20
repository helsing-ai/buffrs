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

use std::{collections::HashMap, fmt, str::FromStr};

use miette::{ensure, miette, Context, IntoDiagnostic, Diagnostic};
use ring::digest;
use semver::Version;
use serde::{de::Visitor, Deserialize, Serialize};
use thiserror::Error;
use tokio::fs;

use crate::{
    errors::{DeserializationError, FileExistsError, FileNotFound, SerializationError, WriteError},
    package::{Package, PackageName},
    registry::RegistryUri,
    ManagedFile,
};

/// File name of the lockfile
pub const LOCKFILE: &str = "Proto.lock";

/// Supported types of digest algorithms
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum DigestAlgorithm {
    // Do not reorder variants: the ordering is significant, see #38 and #106.
    /// 256 bits variant of SHA-2
    #[serde(rename = "sha256")]
    SHA256,
}

/// Error parsing digest algorithm
#[derive(Error, Debug, Diagnostic)]
pub enum DigestAlgorithmError {
    /// Represents a ring digest algorithm that isn't supported by Buffrs
    #[error("unsupported digest algorithm: {0}")]
    UnsupportedAlgorithm(String),
}

impl FromStr for DigestAlgorithm {
    type Err = DigestAlgorithmError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        match serde_typename::from_str(input) {
            Ok(value) => Ok(value),
            _other => Err(DigestAlgorithmError::UnsupportedAlgorithm(input.into())),
        }
    }
}

impl fmt::Display for DigestAlgorithm {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        match serde_typename::to_str(self) {
            Ok(name) => fmt.write_str(name),
            Err(error) => unreachable!("cannot convert DigestAlgorithm to string: {error}"),
        }
    }
}

/// A representation of a cryptographic digest for data integrity validation
#[derive(Clone, PartialEq, Eq, Ord, PartialOrd)]
pub struct Digest {
    // Do not reorder fields: the ordering is significant, see #38 and #106.
    algorithm: DigestAlgorithm,
    bytes: Vec<u8>,
}

impl TryFrom<digest::Digest> for Digest {
    type Error = DigestAlgorithmError;

    fn try_from(value: digest::Digest) -> Result<Self, Self::Error> {
        let algorithm = if value.algorithm() == &digest::SHA256 {
            DigestAlgorithm::SHA256
        } else {
            return Err(DigestAlgorithmError::UnsupportedAlgorithm(format!("{:?}", value.algorithm())));
        };

        Ok(Self {
            bytes: value.as_ref().to_vec(),
            algorithm,
        })
    }
}

impl fmt::Display for Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.algorithm, hex::encode(&self.bytes))
    }
}

impl Serialize for Digest {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.to_string().serialize(serializer)
    }
}

struct DigestVisitor;

impl<'de> Visitor<'de> for DigestVisitor {
    type Value = Digest;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("a hexadecimal encoded cryptographic digest")
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        let mut parts = value.split(':');
        let algorithm_tag = parts.next().ok_or(E::missing_field("algorithm"))?;
        let algorithm = algorithm_tag
            .parse::<DigestAlgorithm>()
            .map_err(|_| E::custom("invalid digest algorithm"))?;
        let bytes = parts
            .next()
            .ok_or(E::missing_field("bytes"))
            .and_then(|h| hex::decode(h).map_err(|_| E::custom("invalid encoding")))?;
        Ok(Self::Value { algorithm, bytes })
    }
}

impl<'de> Deserialize<'de> for Digest {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_str(DigestVisitor)
    }
}

/// Captures immutable metadata about a given package
///
/// It is used to ensure that future installations will use the exact same dependencies.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, PartialOrd, Ord)]
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
    ///
    /// Note that despite returning a Result this function never fails
    pub fn lock(
        package: &Package,
        registry: RegistryUri,
        repository: String,
        dependants: usize,
    ) -> miette::Result<Self> {
        let digest = digest::digest(&digest::SHA256, &package.tgz)
            .try_into()
            .into_diagnostic()
            .wrap_err(miette!("unexpected error: only SHA256 is supported"))?;

        Ok(Self {
            name: package.name().to_owned(),
            registry,
            repository,
            digest,
            version: package.version().to_owned(),
            dependencies: package
                .manifest
                .dependencies
                .iter()
                .map(|d| d.package.clone())
                .collect(),
            dependants,
        })
    }

    /// Validates if another LockedPackage matches this one
    pub fn validate(&self, package: &Package) -> miette::Result<()> {
        let digest: Digest = digest::digest(&digest::SHA256, &package.tgz)
            .try_into()
            .unwrap();

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
