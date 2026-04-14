use crate::{VirtualFileSystem, with_test_registry};

/// Regression test for path canonicalization in workspace publishing.
///
/// When a workspace member (consumer) depends on two APIs (api-a, api-b)
/// that both depend on the same shared library (shared-lib), the publish
/// process resolves shared-lib via different relative paths:
///
///   - consumer/../api-a/../shared-lib (via api-a's dependency chain)
///   - consumer/../api-b/../shared-lib (via api-b's dependency chain)
///
/// Without path canonicalization, these are treated as different keys in
/// the manifest_mappings HashMap, causing the second lookup to fail with
/// "local dependency should have been made available during publish".
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
            .success();
    });
}
