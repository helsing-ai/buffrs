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

use std::{collections::HashMap, str::FromStr};

use ring::digest;
use semver::Version;
use serde::{de::Visitor, Deserialize, Serialize};
use thiserror::Error;
use tokio::fs;

use crate::{
    errors::IoError,
    package::{Package, PackageName},
    registry::RegistryUri,
};

/// File name of the lockfile
pub const LOCKFILE: &str = "Proto.lock";
const SHA256_TAG: &str = "sha256";

/// Supported types of digest algorithms
#[derive(Clone, PartialEq, Eq)]
pub enum DigestAlgorithm {
    /// 256 bits variant of SHA-2
    SHA256,
}

impl std::fmt::Display for DigestAlgorithm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::SHA256 => SHA256_TAG,
        };
        f.write_str(s)
    }
}

/// Error produced when converting into the local digest representation
#[derive(Error, Debug)]
pub enum DigestConversionError {
    /// Produced when the source digest uses an unsupported algorithm
    #[error("Unsupported digest algorithm: {0}")]
    UnsupportedAlgorithm(String),
}

impl FromStr for DigestAlgorithm {
    type Err = DigestConversionError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            SHA256_TAG => Ok(Self::SHA256),
            other => Err(DigestConversionError::UnsupportedAlgorithm(other.into())),
        }
    }
}

impl Ord for DigestAlgorithm {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        match self {
            DigestAlgorithm::SHA256 => match other {
                DigestAlgorithm::SHA256 => std::cmp::Ordering::Equal,
            },
        }
    }
}

impl PartialOrd for DigestAlgorithm {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct Digest {
    bytes: Vec<u8>,
    algorithm: DigestAlgorithm,
}

impl TryFrom<digest::Digest> for Digest {
    type Error = DigestConversionError;

    fn try_from(value: digest::Digest) -> Result<Self, Self::Error> {
        let algorithm = if value.algorithm() == &digest::SHA256 {
            DigestAlgorithm::SHA256
        } else {
            return Err(DigestConversionError::UnsupportedAlgorithm(format!(
                "{:?}",
                value.algorithm()
            )));
        };

        Ok(Self {
            bytes: value.as_ref().to_vec(),
            algorithm,
        })
    }
}

impl std::fmt::Display for Digest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!(
            "{}:{}",
            self.algorithm,
            hex::encode(&self.bytes)
        ))
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

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
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

impl Ord for Digest {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        match self.algorithm.cmp(&other.algorithm) {
            std::cmp::Ordering::Equal => self.bytes.cmp(&other.bytes),
            other => other,
        }
    }
}

impl PartialOrd for Digest {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// Captures immutable metadata about a given package
///
/// It is used to ensure that future installations will use the exact same dependencies.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct LockedPackage {
    pub name: PackageName,
    pub digest: Digest,
    pub registry: RegistryUri,
    pub repository: String,
    pub version: Version,
    pub dependencies: Vec<PackageName>,
    pub dependants: usize,
}

#[derive(Error, Debug)]
#[error("{property} mismatch - expected {expected}, actual {actual}")]
pub struct ValidationError {
    property: &'static str,
    expected: String,
    actual: String,
}

impl LockedPackage {
    /// Captures the source, version and checksum of a Package for use in reproducible installs
    pub fn lock(
        package: &Package,
        registry: RegistryUri,
        repository: String,
        dependants: usize,
    ) -> Self {
        let digest = digest::digest(&digest::SHA256, &package.tgz)
            .try_into()
            .expect("Unexpected error: only SHA256 is supported");

        Self {
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
        }
    }

    /// Validates if another LockedPackage matches this one
    pub fn validate(&self, package: &Package) -> Result<(), ValidationError> {
        let digest: Digest = digest::digest(&digest::SHA256, &package.tgz)
            .try_into()
            .unwrap();

        if &self.name != package.name() {
            return Err(ValidationError {
                property: "Name",
                expected: self.name.to_string(),
                actual: package.name().to_string(),
            });
        }

        if &self.version != package.version() {
            return Err(ValidationError {
                property: "Version",
                expected: self.version.to_string(),
                actual: package.version().to_string(),
            });
        }

        if self.digest != digest {
            return Err(ValidationError {
                property: "Digest",
                expected: self.digest.to_string(),
                actual: digest.to_string(),
            });
        }

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

#[derive(Error, Debug)]
pub enum ReadError {
    #[error("{0}")]
    Io(IoError),
    #[error("Failed to decode lockfile. Cause: {0}")]
    Toml(toml::de::Error),
}

#[derive(Error, Debug)]
pub enum WriteError {
    #[error("{0}")]
    Io(IoError),
    #[error("Failed to serialize lockfile. Cause: {0}")]
    Toml(toml::ser::Error),
}

impl Lockfile {
    /// Checks if the Lockfile currently exists in the filesystem
    pub async fn exists() -> Result<bool, IoError> {
        fs::try_exists(LOCKFILE)
            .await
            .map_err(|err| IoError::new(err, "Failed to detect lockfile"))
    }

    /// Loads the Lockfile from the current directory
    pub async fn read() -> Result<Self, ReadError> {
        let toml = fs::read_to_string(LOCKFILE)
            .await
            .map_err(|err| ReadError::Io(IoError::new(err, "Failed to read lockfile")))?;

        let raw: RawLockfile = toml::from_str(&toml).map_err(ReadError::Toml)?;

        Ok(Self::from_iter(raw.packages.into_iter()))
    }

    /// Loads the Lockfile from the current directory, if it exists, otherwise returns an empty one
    pub async fn read_or_default() -> Result<Self, ReadError> {
        if Lockfile::exists().await.map_err(ReadError::Io)? {
            Lockfile::read().await
        } else {
            Ok(Lockfile::default())
        }
    }

    /// Persists a Lockfile to the filesystem
    pub async fn write(&self) -> Result<(), WriteError> {
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
                .map_err(WriteError::Toml)?
                .into_bytes(),
        )
        .await
        .map_err(|err| WriteError::Io(IoError::new(err, "Failed to write lockfile")))
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
