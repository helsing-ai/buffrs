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

use std::{collections::HashMap, sync::Arc};

use eyre::{ensure, Context};
use ring::digest;
use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};
use tokio::fs;

use crate::{
    manifest::{Dependency, DependencyManifest},
    package::{Package, PackageName},
};

pub const LOCKFILE: &str = "Proto.lock";

/// Captures immutable metadata about a given package
///
/// It is used to ensure that future installations will use the exact same dependencies.
#[derive(Serialize, Deserialize, Clone)]
pub struct LockedPackage {
    pub name: PackageName,
    pub checksum: String,
    pub repository: String,
    pub version: Version,
    pub dependencies: Vec<PackageName>,
}

impl LockedPackage {
    /// Captures the source, version and checksum of a Package for use in reproducible installs
    pub fn lock(package: &Package, repository: String) -> Self {
        let checksum = digest::digest(&digest::SHA256, &package.tgz);

        Self {
            name: package.name().to_owned(),
            repository,
            checksum: hex::encode(checksum),
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
            self.checksum == other.checksum,
            "Checksum mismatch - expected {}, actual {}",
            self.checksum,
            other.checksum
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
    packages: HashMap<PackageName, Arc<LockedPackage>>,
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
        let raw = RawLockfile {
            packages: self
                .packages
                .values()
                .map(|pkg| LockedPackage::clone(pkg))
                .collect(),
        };

        fs::write(LOCKFILE, toml::to_string(&raw)?.into_bytes())
            .await
            .wrap_err("Failed to write lockfile")
    }

    /// Locates a given package in the Lockfile
    pub fn get(&self, name: &PackageName) -> Option<Arc<LockedPackage>> {
        self.packages.get(name).cloned()
    }
}

impl FromIterator<LockedPackage> for Lockfile {
    fn from_iter<I: IntoIterator<Item = LockedPackage>>(iter: I) -> Self {
        Self {
            packages: iter
                .into_iter()
                .map(|locked| (locked.name.clone(), Arc::new(locked)))
                .collect(),
        }
    }
}
