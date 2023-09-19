// (c) Copyright 2023 Helsing GmbH. All rights reserved.

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

#[derive(Serialize, Deserialize, Clone)]
pub struct LockedPackage {
    pub name: PackageName,
    pub checksum: String,
    pub repository: String,
    pub version: Version,
    pub dependencies: Vec<PackageName>,
}

impl LockedPackage {
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

#[derive(Default)]
pub struct Lockfile {
    packages: HashMap<PackageName, Arc<LockedPackage>>,
}

impl Lockfile {
    pub async fn exists() -> eyre::Result<bool> {
        fs::try_exists(LOCKFILE)
            .await
            .wrap_err("Failed to detect lockfile")
    }

    pub async fn read() -> eyre::Result<Self> {
        let toml = fs::read_to_string(LOCKFILE)
            .await
            .wrap_err("Failed to read lockfile")?;

        let raw: RawLockfile = toml::from_str(&toml).wrap_err("Failed to parse lockfile")?;

        Ok(Self {
            packages: raw
                .packages
                .into_iter()
                .map(|locked| (locked.name.clone(), Arc::new(locked)))
                .collect(),
        })
    }

    pub async fn read_or_default() -> eyre::Result<Self> {
        if Lockfile::exists().await? {
            Lockfile::read().await
        } else {
            Ok(Lockfile::default())
        }
    }

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
