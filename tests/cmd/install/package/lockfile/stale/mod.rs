use crate::{VirtualFileSystem, with_authenticated_test_registry};

/// Regression test for a bug where `buffrs install` fails with
/// "unauthorized - please provide registry credentials" when the
/// lockfile is stale (i.e. a new dependency was added to Proto.toml
/// after the lockfile was created).
///
/// Repro:
///   1. Have an existing Proto.toml + Proto.lock that are in sync
///   2. Add a new dependency to Proto.toml
///   3. `buffrs install` should succeed, resolving the new dependency
///   4. Proto.lock should be updated to include the new dependency
#[test]
fn fixture() {
    with_authenticated_test_registry(|url, token| {
        let vfs = VirtualFileSystem::copy(crate::parent_directory!().join("in"));
        let buffrs_home = vfs.root().join("$HOME");
        let cwd = vfs.root();

        // Login to the authenticated registry
        crate::cli!()
            .args(["login", "--registry", url])
            .env("BUFFRS_HOME", &buffrs_home)
            .write_stdin(format!("{token}\n"))
            .assert()
            .success();

        // Create and publish remote-lib-a to the test registry
        {
            std::fs::create_dir(cwd.join("remote-lib-a")).unwrap();
            let lib_dir = cwd.join("remote-lib-a");

            crate::cli!()
                .args(["init", "--lib", "remote-lib-a"])
                .env("BUFFRS_HOME", &buffrs_home)
                .current_dir(&lib_dir)
                .assert()
                .success();

            std::fs::write(
                lib_dir.join("proto/remote_a.proto"),
                "syntax = \"proto3\";\n\npackage remote_a;\n\nmessage DataA {\n  string value = 1;\n}\n",
            )
            .unwrap();

            crate::cli!()
                .args(["publish", "--registry", url, "--repository", "test-repo"])
                .env("BUFFRS_HOME", &buffrs_home)
                .current_dir(&lib_dir)
                .assert()
                .success();
        }

        // Create and publish remote-lib-b to the test registry
        {
            std::fs::create_dir(cwd.join("remote-lib-b")).unwrap();
            let lib_dir = cwd.join("remote-lib-b");

            crate::cli!()
                .args(["init", "--lib", "remote-lib-b"])
                .env("BUFFRS_HOME", &buffrs_home)
                .current_dir(&lib_dir)
                .assert()
                .success();

            std::fs::write(
                lib_dir.join("proto/remote_b.proto"),
                "syntax = \"proto3\";\n\npackage remote_b;\n\nmessage DataB {\n  string value = 1;\n}\n",
            )
            .unwrap();

            crate::cli!()
                .args(["publish", "--registry", url, "--repository", "test-repo"])
                .env("BUFFRS_HOME", &buffrs_home)
                .current_dir(&lib_dir)
                .assert()
                .success();
        }

        // Add remote-lib-a and install to create a synced lockfile
        crate::cli!()
            .args(["add", "--registry", url, "test-repo/remote-lib-a@=0.1.0"])
            .env("BUFFRS_HOME", &buffrs_home)
            .current_dir(&cwd)
            .assert()
            .success();

        crate::cli!()
            .arg("install")
            .env("BUFFRS_HOME", &buffrs_home)
            .current_dir(&cwd)
            .assert()
            .success();

        // Verify the lockfile was created and is in sync
        let lockfile_path = cwd.join("Proto.lock");
        assert!(
            lockfile_path.exists(),
            "Proto.lock should exist after first install"
        );

        let lockfile_after_first = std::fs::read_to_string(&lockfile_path).unwrap();
        assert!(
            lockfile_after_first.contains("remote-lib-a"),
            "Proto.lock should contain remote-lib-a after first install"
        );

        // Now add remote-lib-b — the lockfile becomes stale
        crate::cli!()
            .args(["add", "--registry", url, "test-repo/remote-lib-b@=0.1.0"])
            .env("BUFFRS_HOME", &buffrs_home)
            .current_dir(&cwd)
            .assert()
            .success();

        // Install again with a stale lockfile — this must succeed
        crate::cli!()
            .arg("install")
            .env("BUFFRS_HOME", &buffrs_home)
            .current_dir(&cwd)
            .assert()
            .success();

        // The lockfile must be updated to include both dependencies
        let lockfile_after_second = std::fs::read_to_string(&lockfile_path).unwrap();
        assert!(
            lockfile_after_second.contains("remote-lib-a"),
            "Proto.lock should still contain remote-lib-a after second install"
        );
        assert!(
            lockfile_after_second.contains("remote-lib-b"),
            "Proto.lock should contain remote-lib-b after second install"
        );
    })
}
