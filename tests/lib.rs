use buffrs::package::Package;

use std::{
    fs,
    path::{Path, PathBuf},
};

use assert_fs::TempDir;
use bytes::Bytes;
use fs_extra::dir::{get_dir_content, CopyOptions};
use pretty_assertions::{assert_eq, assert_str_eq};

mod cmd;
mod test_registry;

pub use test_registry::with_test_registry;

/// Create a command which runs the cli
#[macro_export]
macro_rules! cli {
    () => {
        assert_cmd::Command::cargo_bin(assert_cmd::crate_name!())
            .unwrap()
            .env(
                "BUFFRS_HOME",
                &format!("./{}", $crate::VirtualFileSystem::VIRTUAL_HOME),
            )
            .env("BUFFRS_TESTSUITE", "1")
    };
}

/// A virtual file system which enables temporary fs operations
pub struct VirtualFileSystem {
    tmp_dir: TempDir,
    virtual_home: bool,
}

impl VirtualFileSystem {
    const VIRTUAL_HOME: &'static str = "$HOME";
    const ROOT_NAME: &'static str = "root";

    /// Init an empty virtual file system
    pub fn empty() -> Self {
        let tmp_dir = TempDir::new().unwrap();
        let root = tmp_dir.join(Self::ROOT_NAME);

        fs_extra::dir::create(&root, false).ok();
        fs_extra::dir::create(root.join(Self::VIRTUAL_HOME), false).ok();

        Self {
            tmp_dir,
            virtual_home: false,
        }
    }

    /// Init a virtual file system from a local directory
    pub fn copy(template: impl AsRef<Path>) -> Self {
        let tmp_dir = TempDir::new().unwrap();
        let root = tmp_dir.join(Self::ROOT_NAME);

        fs_extra::dir::create(&root, false).ok();

        fs_extra::dir::copy(
            template.as_ref(),
            &root,
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

        fs_extra::dir::create(root.join(Self::VIRTUAL_HOME), false).ok();

        Self {
            tmp_dir,
            virtual_home: false,
        }
    }

    /// Root path to run operations in
    pub fn root(&self) -> PathBuf {
        self.tmp_dir.join(Self::ROOT_NAME)
    }

    /// Enable verification of the virtual home
    pub fn with_virtual_home(mut self) -> Self {
        self.virtual_home = true;
        self
    }

    /// Verify the virtual file system to be equal to a local directory
    pub fn verify_against(&self, expected: impl AsRef<Path>) {
        let vfs = get_dir_content(self.root()).unwrap();
        let exp = get_dir_content(expected.as_ref()).unwrap();

        let files = {
            let filter_vhome = |f: &PathBuf| {
                if self.virtual_home {
                    true
                } else {
                    !f.starts_with(Self::VIRTUAL_HOME)
                }
            };

            let filter_gitkeep = |f: &PathBuf| !f.ends_with(".gitkeep");

            let filter_proto_toml_orig = |f: &PathBuf| !f.ends_with("Proto.toml.orig");

            let mut actual_files: Vec<PathBuf> = vfs
                .files
                .iter()
                .map(Path::new)
                .map(|f| f.strip_prefix(self.root()).unwrap().to_path_buf())
                .filter(filter_vhome)
                .filter(filter_gitkeep)
                .filter(filter_proto_toml_orig)
                .collect();

            actual_files.sort();

            let mut expected_files: Vec<PathBuf> = exp
                .files
                .iter()
                .map(Path::new)
                .map(|f| f.strip_prefix(expected.as_ref()).unwrap().to_path_buf())
                .filter(filter_vhome)
                .filter(filter_gitkeep)
                .filter(filter_proto_toml_orig)
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

            if let Some(extension) = file.extension() {
                match FileType::from_extension(extension.to_str().unwrap()) {
                    FileType::Text => {
                        assert_str_eq!(
                            fs::read_to_string(&expected).expect("file cannot be read"),
                            fs::read_to_string(&actual).expect("file cannot be read")
                        );
                    }
                    FileType::Package => {
                        fn read_package(path: &PathBuf) -> Package {
                            Bytes::from(fs::read(path).expect("file cannot be read"))
                                .try_into()
                                .expect("package could not be parsed")
                        }
                        let actual = read_package(&actual);
                        let expected = read_package(&expected);
                        let actual_vfs = VirtualFileSystem::empty();
                        let expected_vfs = VirtualFileSystem::empty();
                        let runtime = tokio::runtime::Builder::new_current_thread()
                            .build()
                            .unwrap();
                        runtime
                            .block_on(actual.unpack(&actual_vfs.root()))
                            .expect("package could not be unpacked");
                        runtime
                            .block_on(expected.unpack(&expected_vfs.root()))
                            .expect("package could not be unpacked");
                        actual_vfs.verify_against(expected_vfs.root());
                    }
                }
            } else {
                panic!("missing file extension");
            }
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

enum FileType {
    Package,
    Text,
}

impl FileType {
    pub fn from_extension(ext: impl AsRef<str>) -> Self {
        match ext.as_ref() {
            "tgz" => Self::Package,
            "proto" | "toml" | "lock" => Self::Text,
            other => panic!("unrecognized extension type: {other}"),
        }
    }
}
