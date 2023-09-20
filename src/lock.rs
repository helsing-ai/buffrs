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

use eyre::{ensure, Context};
use ring::digest;
use semver::{Version, VersionReq};
use serde::{de::Visitor, Deserialize, Serialize};
use tokio::fs;

use crate::{
    manifest::{Dependency, DependencyManifest},
    package::{Package, PackageName},
    registry::RegistryUri,
};

pub const LOCKFILE: &str = "Proto.lock";

const SHA256_TAG: &str = "sha256";

#[derive(Clone, PartialEq, Eq)]
pub enum DigestAlgorithm {
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

impl FromStr for DigestAlgorithm {
    type Err = eyre::Report;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            SHA256_TAG => Ok(Self::SHA256),
            other => eyre::bail!("invalid digest algorithm: {other}"),
        }
    }
}

#[derive(Clone)]
pub struct Digest {
    bytes: Vec<u8>,
    algorithm: DigestAlgorithm,
}

impl TryFrom<digest::Digest> for Digest {
    type Error = eyre::Report;

    fn try_from(value: digest::Digest) -> Result<Self, Self::Error> {
        let algorithm = if value.algorithm() == &digest::SHA256 {
            DigestAlgorithm::SHA256
        } else {
            eyre::bail!("Unsupported digest algorithm: {:?}", value.algorithm())
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

impl PartialEq for Digest {
    fn eq(&self, other: &Self) -> bool {
        self.algorithm == other.algorithm && self.bytes == other.bytes
    }
}

impl Eq for Digest {}

impl Ord for Digest {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.bytes.cmp(&other.bytes)
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
}

impl LockedPackage {
    /// Captures the source, version and checksum of a Package for use in reproducible installs
    pub fn lock(package: &Package, registry: RegistryUri, repository: String) -> Self {
        let digest = digest::digest(&digest::SHA256, &package.tgz);

        Self {
            name: package.name().to_owned(),
            registry,
            repository,
            digest: digest.try_into().unwrap(), // won't fail as we SHA256 is sure to be supported
            version: package.version().to_owned(),
            dependencies: package
                .manifest
                .dependencies
                .iter()
                .cloned()
                .map(|d| d.package)
                .collect(),
        }
    }

    /// Validates if another LockedPackage matches this one
    pub fn validate(&self, other: &Self) -> eyre::Result<()> {
        ensure!(
            self.name == other.name,
            "Package name mismatch - expected {}, actual {}",
            self.name,
            other.name
        );
        ensure!(
            self.digest == other.digest,
            "Digest mismatch - expected {}, actual {}",
            self.digest,
            other.digest
        );
        ensure!(
            self.registry == other.registry,
            "Registry mismatch - expected {}, actual {}",
            self.registry,
            other.registry
        );
        ensure!(
            self.repository == other.repository,
            "Repository mismatch - expected {}, actual {}",
            self.repository,
            other.repository
        );
        ensure!(
            self.version == other.version,
            "Version mismatch - expected {}, actual {}",
            self.version,
            other.version
        );
        ensure!(
            self.dependencies == other.dependencies,
            "Dependencies mismatch"
        );
        Ok(())
    }

    /// Constructs a Dependency instance with matching metadata
    pub fn as_dependency(&self) -> Dependency {
        Dependency {
            package: self.name.clone(),
            manifest: DependencyManifest {
                version: VersionReq {
                    comparators: vec![semver::Comparator {
                        op: semver::Op::Exact,
                        major: self.version.major,
                        minor: Some(self.version.minor),
                        patch: Some(self.version.patch),
                        pre: self.version.pre.clone(),
                    }],
                },
                registry: self.registry.clone(),
                repository: self.repository.clone(),
            },
        }
    }
}

#[derive(Serialize, Deserialize)]
struct RawLockfile {
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
    pub async fn exists() -> eyre::Result<bool> {
        fs::try_exists(LOCKFILE)
            .await
            .wrap_err("Failed to detect lockfile")
    }

    /// Loads the Lockfile from the current directory
    pub async fn read() -> eyre::Result<Self> {
        let toml = fs::read_to_string(LOCKFILE)
            .await
            .wrap_err("Failed to read lockfile")?;

        let raw: RawLockfile = toml::from_str(&toml).wrap_err("Failed to parse lockfile")?;

        Ok(Self::from_iter(raw.packages.into_iter()))
    }

    /// Loads the Lockfile from the current directory, if it exists, otherwise returns an empty one
    pub async fn read_or_default() -> eyre::Result<Self> {
        if Lockfile::exists().await? {
            Lockfile::read().await
        } else {
            Ok(Lockfile::default())
        }
    }

    /// Persists a Lockfile to the filesystem
    pub async fn write(&self) -> eyre::Result<()> {
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

        let raw = RawLockfile { packages };

        fs::write(LOCKFILE, toml::to_string(&raw)?.into_bytes())
            .await
            .wrap_err("Failed to write lockfile")
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
