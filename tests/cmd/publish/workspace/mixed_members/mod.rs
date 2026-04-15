use crate::{VirtualFileSystem, with_test_registry};

/// Test that a workspace with a mix of publishable and dependency-only
/// members succeeds: publishable members (with `[package]`) are published
/// while dependency-only members (without `[package]`) are silently skipped.
#[test]
fn fixture() {
    with_test_registry(|url| {
        let vfs = VirtualFileSystem::copy(crate::parent_directory!().join("in"));

        crate::cli!()
            .arg("publish")
            .arg("--registry")
            .arg(url)
            .arg("--repository")
            .arg("my-repository")
            .arg("--set-version")
            .arg("1.0.0")
            .current_dir(vfs.root())
            .assert()
            .success()
            .stdout(include_str!("stdout.log"))
            .stderr(include_str!("stderr.log"));
    });
}
