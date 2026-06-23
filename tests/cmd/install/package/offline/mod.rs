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

        // Install with --offline and an empty cache — should fail
        let output = crate::cli!()
            .args(["install", "--offline"])
            .env("BUFFRS_HOME", &buffrs_home)
            .env("BUFFRS_CACHE", cwd.join("empty-cache"))
            .current_dir(&cwd)
            .assert()
            .failure();

        let stderr = String::from_utf8_lossy(&output.get_output().stderr);
        assert!(
            stderr.contains("offline"),
            "expected 'offline' in error output, got:\n{stderr}"
        );
    });
}
