use std::{
    fs,
    path::{Path, PathBuf},
};

use assert_cmd::Command;
use assert_fs::TempDir;
use fs_extra::dir::{get_dir_content, CopyOptions};
use pretty_assertions::{assert_eq, assert_str_eq};

mod cmd;

/// Create a command which runs the cli
pub fn cli() -> assert_cmd::Command {
    Command::cargo_bin(assert_cmd::crate_name!()).unwrap()
}

/// A virtual file system which enables temporary fs operations
pub struct VirtualFileSystem(TempDir);

impl VirtualFileSystem {
    /// Init an empty virtual file system
    pub fn empty() -> Self {
        Self(TempDir::new().unwrap())
    }

    /// Init a virtual file system from a local directory
    pub fn copy(template: impl AsRef<Path>) -> Self {
        let cwd = TempDir::new().unwrap();
        fs_extra::dir::copy(
            template.as_ref(),
            cwd.path(),
            &CopyOptions {
                overwrite: true,
                skip_exist: false,
                buffer_size: 8192,
                copy_inside: true,
                content_only: true,
                depth: 64,
            },
        )
        .unwrap();
        Self(cwd)
    }

    /// Root path to run operations in
    pub fn root(&self) -> &Path {
        self.0.path()
    }

    /// Verify the virtual file system to be equal to a local directory
    pub fn verify_against(&self, expected: impl AsRef<Path>) {
        let vfs = get_dir_content(self.root()).unwrap();
        let exp = get_dir_content(expected.as_ref()).unwrap();

        let files = {
            let mut actual_files: Vec<PathBuf> = vfs
                .files
                .iter()
                .map(Path::new)
                .map(|f| f.strip_prefix(self.root()).unwrap().to_path_buf())
                .collect();

            actual_files.sort();

            let mut expected_files: Vec<PathBuf> = exp
                .files
                .iter()
                .map(Path::new)
                .map(|f| f.strip_prefix(expected.as_ref()).unwrap().to_path_buf())
                .collect();

            expected_files.sort();

            assert_eq!(
                expected_files, actual_files,
                "found difference in directory structure"
            );

            actual_files
        };

        for file in files {
            let actual = self.root().join(&file);
            let expected = expected.as_ref().join(&file);

            println!("\n-- {} –-\n", file.display());

            assert_str_eq!(
                &fs::read_to_string(&expected).expect("file not found"),
                &fs::read_to_string(&actual).expect("file not found"),
            );
        }
    }
}

impl Drop for VirtualFileSystem {
    fn drop(&mut self) {
        fs_extra::dir::remove(self.root()).expect("failed to cleanup vfs");
    }
}

#[macro_export]
macro_rules! parent_directory {
    () => {{
        std::path::Path::new(file!()).parent().unwrap()
    }};
}
