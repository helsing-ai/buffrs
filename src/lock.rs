// (c) Copyright 2023 Helsing GmbH. All rights reserved.

use eyre::Context;
use ring::digest;
use semver::Version;
use serde::{Deserialize, Serialize};
use tokio::fs;

use crate::package::{Package, PackageName};

pub const LOCKFILE: &str = "Proto.lock";

#[derive(Serialize, Deserialize)]
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
}

#[derive(Serialize, Deserialize)]
pub struct Lockfile {
    packages: Vec<LockedPackage>,
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

        let lock: Self = toml::from_str(&toml).wrap_err("Failed to parse lockfile")?;

        Ok(lock)
    }

    pub async fn write(&self) -> eyre::Result<()> {
        fs::write(LOCKFILE, toml::to_string(&self)?.into_bytes())
            .await
            .wrap_err("Failed to write lockfile")
    }
}
