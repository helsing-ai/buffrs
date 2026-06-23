use crate::{VirtualFileSystem, with_test_registry};

#[test]
fn fixture() {
    with_test_registry(|url| {
        let vfs = VirtualFileSystem::copy(crate::parent_directory!().join("in"));
        let buffrs_home = vfs.root().join("$HOME");
        let cwd = vfs.root();

        // Publish a remote-lib to the registry
        crate::publish_test_library(
            &cwd,
            &buffrs_home,
            url,
            "test-repo",
            "remote-lib",
            None,
            "remote.proto",
            "syntax = \"proto3\";\n\npackage remote;\n\nmessage Data {\n  string value = 1;\n}\n",
        );

        // Add the dependency
        crate::cli!()
            .args(["add", "--registry", url, "test-repo/remote-lib@=0.1.0"])
            .env("BUFFRS_HOME", &buffrs_home)
            .current_dir(&cwd)
            .assert()
            .success();

        // Install without --offline (online mode) and an empty cache — should succeed by downloading
        crate::cli!()
            .args(["install"])
            .env("BUFFRS_HOME", &buffrs_home)
            .env("BUFFRS_CACHE", cwd.join("empty-cache"))
            .current_dir(&cwd)
            .assert()
            .success();

        // Verify that the package was installed
        assert!(
            cwd.join("proto/vendor/remote-lib/remote.proto").exists(),
            "Expected remote-lib to be installed in vendor directory"
        );
    });
}
