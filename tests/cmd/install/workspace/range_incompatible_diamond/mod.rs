use crate::{VirtualFileSystem, with_test_registry};

/// Verifies that incompatible range requirements on the same transitive leaf
/// within a single package's dependency graph cause `buffrs install` to fail
/// with a clear version-conflict error.
///
/// Diamond shape (all within pkg1):
///   pkg1 --(^1.0)--> lib-a --(^1.0.0)--> leaf-lib  (resolves to v1.0.0)
///   pkg1 --(^1.0)--> lib-b --(^2.0.0)--> leaf-lib  (requires v2.x — conflict)
///
/// No single version of leaf-lib satisfies both ^1.0.0 and ^2.0.0.
/// The resolver encounters leaf-lib a second time via lib-b, calls
/// validate_version_compatibility, and must reject with a version-conflict error.
#[test]
fn fixture() {
    with_test_registry(|url| {
        let vfs = VirtualFileSystem::copy(crate::parent_directory!().join("in"));
        let buffrs_home = vfs.root().join("$HOME");
        let cwd = vfs.root();

        // Publish leaf-lib at v1.0.0
        {
            let lib_dir = cwd.join("leaf-lib-v1");
            std::fs::create_dir(&lib_dir).unwrap();

            crate::cli!()
                .args(["init", "--lib", "leaf-lib"])
                .env("BUFFRS_HOME", &buffrs_home)
                .current_dir(&lib_dir)
                .assert()
                .success();

            let manifest_path = lib_dir.join("Proto.toml");
            let manifest = std::fs::read_to_string(&manifest_path).unwrap();
            let updated = manifest.replace("version = \"0.1.0\"", "version = \"1.0.0\"");
            std::fs::write(&manifest_path, updated).unwrap();

            std::fs::write(
                lib_dir.join("proto/leaf.proto"),
                "syntax = \"proto3\";\n\npackage leaf;\n\nmessage LeafMessage {\n  string value = 1;\n}\n",
            )
            .unwrap();

            crate::cli!()
                .args(["publish", "--registry", url, "--repository", "test-repo"])
                .env("BUFFRS_HOME", &buffrs_home)
                .current_dir(&lib_dir)
                .assert()
                .success();
        }

        // Publish leaf-lib at v2.0.0
        {
            let lib_dir = cwd.join("leaf-lib-v2");
            std::fs::create_dir(&lib_dir).unwrap();

            crate::cli!()
                .args(["init", "--lib", "leaf-lib"])
                .env("BUFFRS_HOME", &buffrs_home)
                .current_dir(&lib_dir)
                .assert()
                .success();

            let manifest_path = lib_dir.join("Proto.toml");
            let manifest = std::fs::read_to_string(&manifest_path).unwrap();
            let updated = manifest.replace("version = \"0.1.0\"", "version = \"2.0.0\"");
            std::fs::write(&manifest_path, updated).unwrap();

            std::fs::write(
                lib_dir.join("proto/leaf.proto"),
                "syntax = \"proto3\";\n\npackage leaf;\n\nmessage LeafV2 {\n  string value = 1;\n}\n",
            )
            .unwrap();

            crate::cli!()
                .args(["publish", "--registry", url, "--repository", "test-repo"])
                .env("BUFFRS_HOME", &buffrs_home)
                .current_dir(&lib_dir)
                .assert()
                .success();
        }

        // Publish lib-a at v1.0.0 (depends on leaf-lib@^1.0.0 — resolves to v1.0.0)
        {
            let lib_dir = cwd.join("lib-a");
            std::fs::create_dir(&lib_dir).unwrap();

            crate::cli!()
                .args(["init", "--lib", "lib-a"])
                .env("BUFFRS_HOME", &buffrs_home)
                .current_dir(&lib_dir)
                .assert()
                .success();

            let manifest_path = lib_dir.join("Proto.toml");
            let manifest = std::fs::read_to_string(&manifest_path).unwrap();
            let updated = manifest.replace("version = \"0.1.0\"", "version = \"1.0.0\"");
            std::fs::write(&manifest_path, updated).unwrap();

            std::fs::write(
                lib_dir.join("proto/liba.proto"),
                "syntax = \"proto3\";\n\npackage liba;\n\nmessage LibAMessage {\n  string value = 1;\n}\n",
            )
            .unwrap();

            crate::cli!()
                .args(["add", "--registry", url, "test-repo/leaf-lib@^1.0.0"])
                .env("BUFFRS_HOME", &buffrs_home)
                .current_dir(&lib_dir)
                .assert()
                .success();

            crate::cli!()
                .args(["publish", "--registry", url, "--repository", "test-repo"])
                .env("BUFFRS_HOME", &buffrs_home)
                .current_dir(&lib_dir)
                .assert()
                .success();
        }

        // Publish lib-b at v1.0.0 (depends on leaf-lib@^2.0.0 — incompatible with ^1.0.0)
        {
            let lib_dir = cwd.join("lib-b");
            std::fs::create_dir(&lib_dir).unwrap();

            crate::cli!()
                .args(["init", "--lib", "lib-b"])
                .env("BUFFRS_HOME", &buffrs_home)
                .current_dir(&lib_dir)
                .assert()
                .success();

            let manifest_path = lib_dir.join("Proto.toml");
            let manifest = std::fs::read_to_string(&manifest_path).unwrap();
            let updated = manifest.replace("version = \"0.1.0\"", "version = \"1.0.0\"");
            std::fs::write(&manifest_path, updated).unwrap();

            std::fs::write(
                lib_dir.join("proto/libb.proto"),
                "syntax = \"proto3\";\n\npackage libb;\n\nmessage LibBMessage {\n  string value = 1;\n}\n",
            )
            .unwrap();

            crate::cli!()
                .args(["add", "--registry", url, "test-repo/leaf-lib@^2.0.0"])
                .env("BUFFRS_HOME", &buffrs_home)
                .current_dir(&lib_dir)
                .assert()
                .success();

            crate::cli!()
                .args(["publish", "--registry", url, "--repository", "test-repo"])
                .env("BUFFRS_HOME", &buffrs_home)
                .current_dir(&lib_dir)
                .assert()
                .success();
        }

        std::fs::remove_dir_all(cwd.join("leaf-lib-v1")).unwrap();
        std::fs::remove_dir_all(cwd.join("leaf-lib-v2")).unwrap();
        std::fs::remove_dir_all(cwd.join("lib-a")).unwrap();
        std::fs::remove_dir_all(cwd.join("lib-b")).unwrap();

        // pkg1 depends on BOTH lib-a and lib-b — the diamond conflict is within
        // a single package's dependency graph, where validate_version_compatibility
        // must reject leaf-lib@^2.0.0 after resolving leaf-lib to 1.0.0 via lib-a.
        crate::cli!()
            .args(["add", "--registry", url, "test-repo/lib-a@^1.0.0"])
            .env("BUFFRS_HOME", &buffrs_home)
            .current_dir(cwd.join("pkg1"))
            .assert()
            .success();

        crate::cli!()
            .args(["add", "--registry", url, "test-repo/lib-b@^1.0.0"])
            .env("BUFFRS_HOME", &buffrs_home)
            .current_dir(cwd.join("pkg1"))
            .assert()
            .success();

        // Install must fail: leaf-lib cannot satisfy both ^1.0.0 and ^2.0.0
        let output = crate::cli!()
            .arg("install")
            .env("BUFFRS_HOME", &buffrs_home)
            .current_dir(&cwd)
            .output()
            .unwrap();

        assert!(
            !output.status.success(),
            "install should fail when two branches of the same package require incompatible \
             versions of leaf-lib (^1.0.0 vs ^2.0.0)"
        );

        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("version conflict for") || stderr.contains("leaf-lib"),
            "error output should mention the version conflict, got:\n{}",
            stderr
        );
    })
}
