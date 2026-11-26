use std::{
    fs::File,
    time::{Duration, SystemTime},
};

use assert_cmd::assert::Assert;

use crate::{VirtualFileSystem, with_test_registry};

/// Publishes a package, changes the mtime of its proto file and publishes again.
///
/// Returns the assertion of the second publish.
fn republish_after_touch(flag: &str) -> Assert {
    let mut republish = None;

    with_test_registry(|url| {
        let vfs = VirtualFileSystem::copy(crate::parent_directory!().join("../lib/in"));

        let publish = || {
            crate::cli!()
                .arg("publish")
                .arg("--registry")
                .arg(url)
                .arg("--repository")
                .arg("my-repository")
                .arg(flag)
                .current_dir(vfs.root())
                .assert()
        };

        let proto = vfs.root().join("proto/lib.proto");
        std::fs::write(&proto, "syntax = \"proto3\";\n\npackage lib;\n").unwrap();

        publish().success();

        File::options()
            .write(true)
            .open(&proto)
            .unwrap()
            .set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000_000))
            .unwrap();

        republish = Some(publish());
    });

    republish.unwrap()
}

#[test]
fn disabled_makes_republish_idempotent() {
    let assert = republish_after_touch("--preserve-mtime=false").success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);

    assert!(
        stdout.contains("is already published, skipping"),
        "{stdout}"
    );
}

#[test]
fn bare_flag_still_preserves_mtime() {
    let assert = republish_after_touch("--preserve-mtime").failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);

    // miette wraps the message, so only match the part that stays on one line.
    assert!(stderr.contains("package is already published"), "{stderr}");
}
