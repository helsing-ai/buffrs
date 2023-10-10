use std::fmt::Display;

use crate::{credentials::CREDENTIALS_FILE, lock::LOCKFILE, manifest::MANIFEST_FILE};

#[derive(Debug)]
pub(crate) enum ManagedFile {
    Credentials,
    Manifest,
    Lock,
}

impl ManagedFile {
    fn name(&self) -> &str {
        match self {
            ManagedFile::Manifest => MANIFEST_FILE,
            ManagedFile::Lock => LOCKFILE,
            ManagedFile::Credentials => CREDENTIALS_FILE,
        }
    }
}

impl Display for ManagedFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}
