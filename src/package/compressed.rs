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

use std::{
    collections::BTreeMap,
    io::{self, BufReader, Cursor, Read, Write},
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

use bytes::{Buf, Bytes};
use miette::{Context, IntoDiagnostic, bail, miette};
use semver::Version;
use tokio::fs;

use crate::{
    ManagedFile,
    errors::{DeserializationError, SerializationError},
    lock::{Digest, DigestAlgorithm, LockedPackage},
    manifest::{self, Edition, MANIFEST_FILE, PackagesManifest},
    package::PackageName,
    package::store::Entry,
    registry::RegistryUri,
};

/// An in memory representation of a `buffrs` package
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Package {
    /// Manifest of the package
    pub manifest: PackagesManifest,
    /// The `tar.gz` archive containing the protocol buffers
    pub tgz: Bytes,
}

impl Package {
    /// Create new [`Package`] from [`Manifest`] and list of files.
    ///
    /// This intentionally uses a [`BTreeMap`] to ensure that the list of files is sorted
    /// lexicographically. This ensures a reproducible output.
    pub fn create(
        manifest: PackagesManifest,
        files: BTreeMap<PathBuf, Entry>,
        preserve_mtime: bool,
    ) -> miette::Result<Self> {
        if manifest.edition == Edition::Unknown {
            // Upgrade unknown edition to latest
            let builder = PackagesManifest::builder();
            if let Some(pkg) = manifest.clone().package {
                builder.package(pkg);
            }
        }

        if manifest.package.is_none() {
            bail!("failed to create package, manifest doesnt contain a package declaration")
        }

        let mut archive = tar::Builder::new(Vec::new());

        let manifest_bytes = {
            let as_str: String = manifest
                .clone()
                .try_into()
                .into_diagnostic()
                .wrap_err(SerializationError(ManagedFile::Manifest))?;

            as_str.into_bytes()
        };

        let mut header = tar::Header::new_gnu();

        header.set_size(
            manifest_bytes
                .len()
                .try_into()
                .into_diagnostic()
                .wrap_err(miette!(
                    "serialized manifest was too large to fit in a tarball"
                ))?,
        );

        header.set_mode(0o444);

        archive
            .append_data(&mut header, MANIFEST_FILE, Cursor::new(manifest_bytes))
            .into_diagnostic()
            .wrap_err(miette!("failed to add manifest to release"))?;

        for (name, entry) in &files {
            let mut header = tar::Header::new_gnu();

            let Entry { contents, metadata } = entry;

            if preserve_mtime {
                let mtime = metadata
                    .as_ref()
                    .and_then(|metadata| metadata.modified().ok())
                    .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
                    .map(|duration| duration.as_secs());

                if let Some(mtime) = mtime {
                    header.set_mtime(mtime);
                }
            }

            header.set_mode(0o444);
            header.set_size(contents.len() as u64);
            archive
                .append_data(&mut header, name, &contents[..])
                .into_diagnostic()
                .wrap_err(miette!("failed to add proto {name:?} to release tar"))?;
        }

        let tar = archive
            .into_inner()
            .into_diagnostic()
            .wrap_err(miette!("failed to assemble tar package"))?;

        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());

        encoder
            .write_all(&tar)
            .into_diagnostic()
            .wrap_err(miette!("failed to compress release"))?;

        let tgz: Bytes = encoder
            .finish()
            .into_diagnostic()
            .wrap_err(miette!("failed to finalize package"))?
            .into();

        Ok(Self { manifest, tgz })
    }

    /// Unpack a package to a specific path.
    pub async fn unpack(&self, path: &Path) -> miette::Result<()> {
        let mut tar = Vec::new();
        let mut gz = flate2::read::GzDecoder::new(self.tgz.clone().reader());

        gz.read_to_end(&mut tar)
            .into_diagnostic()
            .wrap_err(miette!("failed to decompress package {}", self.name()))?;

        let mut tar = tar::Archive::new(Bytes::from(tar).reader());

        fs::remove_dir_all(path).await.ok();

        fs::create_dir_all(path).await.into_diagnostic().wrap_err({
            miette!(
                "failed to create extraction directory for package {}",
                self.name()
            )
        })?;

        tar.unpack(path).into_diagnostic().wrap_err({
            miette!(
                "failed to extract package {} to {}",
                self.name(),
                path.display()
            )
        })?;

        Ok(())
    }

    /// Load a package from a precompressed archive.
    pub(crate) fn parse(tgz: Bytes) -> miette::Result<Self> {
        let mut tar = Vec::new();

        let mut gz = flate2::read::GzDecoder::new(tgz.clone().reader());

        gz.read_to_end(&mut tar)
            .into_diagnostic()
            .wrap_err(miette!("failed to decompress package"))?;

        let mut tar = tar::Archive::new(Bytes::from(tar).reader());

        let manifest = tar
            .entries()
            .into_diagnostic()
            .wrap_err(miette!("corrupted tar package"))?
            .filter_map(|entry| entry.ok())
            .find(|entry| {
                entry
                    .path()
                    .ok()
                    // TODO(rfink): The following line is a bug since it checks whether
                    //  actual path (relative to the process pwd) is a file, *not* whether
                    //  the tar entry would be a file if unpacked
                    // .filter(|path| path.is_file())
                    .filter(|path| path.ends_with(manifest::MANIFEST_FILE))
                    .is_some()
            })
            .ok_or_else(|| miette!("missing manifest"))?;

        let manifest = BufReader::new(manifest);

        let manifest = manifest
            .bytes()
            .collect::<io::Result<Vec<_>>>()
            .into_diagnostic()
            .wrap_err(DeserializationError(ManagedFile::Manifest))?;

        let manifest = String::from_utf8(manifest)
            .into_diagnostic()
            .wrap_err(miette!("manifest has invalid character encoding"))?
            .parse()?;

        Ok(Self { manifest, tgz })
    }

    /// The name of this package
    #[inline]
    pub fn name(&self) -> &PackageName {
        assert!(self.manifest.package.is_some());

        &self
            .manifest
            .package
            .as_ref()
            .expect("compressed package contains invalid manifest (package section missing)")
            .name
    }

    /// The version of this package
    #[inline]
    pub fn version(&self) -> &Version {
        assert!(self.manifest.package.is_some());

        &self
            .manifest
            .package
            .as_ref()
            .expect("compressed package contains invalid manifest (package section missing)")
            .version
    }

    /// Digest calculates the digest based on the downloaded package bytes
    pub fn digest(&self, algorithm: DigestAlgorithm) -> Digest {
        algorithm.digest(&self.tgz)
    }

    /// Lock this package
    pub fn lock(
        &self,
        registry: RegistryUri,
        repository: String,
        dependants: usize,
    ) -> LockedPackage {
        LockedPackage::lock(self, registry, repository, dependants)
    }
}

impl TryFrom<Bytes> for Package {
    type Error = miette::Report;

    fn try_from(tgz: Bytes) -> Result<Self, Self::Error> {
        Package::parse(tgz)
    }
}
