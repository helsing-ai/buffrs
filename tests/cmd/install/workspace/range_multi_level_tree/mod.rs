use crate::{VirtualFileSystem, with_test_registry};

/// Verifies that range resolution propagates correctly through a three-level
/// dependency tree: pkg1 --(^2.0)--> mid-lib --(^1.0)--> leaf-lib.
///
/// Publishes leaf-lib at v1.0.0 and v1.5.0, then mid-lib at v2.0.0 (which
/// declares `leaf-lib@^1.0.0`), then installs pkg1 with `mid-lib@^2.0.0`.
/// Expects mid-lib pinned at 2.0.0 and leaf-lib pinned at 1.5.0.
#[test]
fn fixture() {
    with_test_registry(|url| {
        let vfs = VirtualFileSystem::copy(crate::parent_directory!().join("in"));
        let buffrs_home = vfs.root().join("$HOME");
        let cwd = vfs.root();

        // Publish leaf-lib at v1.0.0
        {
            let lib_dir = cwd.join("leaf-lib-v1-0");
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

        // Publish leaf-lib at v1.5.0 (should be resolved by mid-lib's ^1.0.0)
        {
            let lib_dir = cwd.join("leaf-lib-v1-5");
            std::fs::create_dir(&lib_dir).unwrap();

            crate::cli!()
                .args(["init", "--lib", "leaf-lib"])
                .env("BUFFRS_HOME", &buffrs_home)
                .current_dir(&lib_dir)
                .assert()
                .success();

            let manifest_path = lib_dir.join("Proto.toml");
            let manifest = std::fs::read_to_string(&manifest_path).unwrap();
            let updated = manifest.replace("version = \"0.1.0\"", "version = \"1.5.0\"");
            std::fs::write(&manifest_path, updated).unwrap();

            std::fs::write(
                lib_dir.join("proto/leaf.proto"),
                "syntax = \"proto3\";\n\npackage leaf;\n\nmessage LeafMessage {\n  string value = 1;\n  int32 count = 2;\n}\n",
            )
            .unwrap();

            crate::cli!()
                .args(["publish", "--registry", url, "--repository", "test-repo"])
                .env("BUFFRS_HOME", &buffrs_home)
                .current_dir(&lib_dir)
                .assert()
                .success();
        }

        // Publish mid-lib at v2.0.0 with a transitive dependency on leaf-lib@^1.0.0
        {
            let lib_dir = cwd.join("mid-lib");
            std::fs::create_dir(&lib_dir).unwrap();

            crate::cli!()
                .args(["init", "--lib", "mid-lib"])
                .env("BUFFRS_HOME", &buffrs_home)
                .current_dir(&lib_dir)
                .assert()
                .success();

            let manifest_path = lib_dir.join("Proto.toml");
            let manifest = std::fs::read_to_string(&manifest_path).unwrap();
            let updated = manifest.replace("version = \"0.1.0\"", "version = \"2.0.0\"");
            std::fs::write(&manifest_path, updated).unwrap();

            std::fs::write(
                lib_dir.join("proto/mid.proto"),
                "syntax = \"proto3\";\n\npackage mid;\n\nmessage MidMessage {\n  string value = 1;\n}\n",
            )
            .unwrap();

            // mid-lib depends on leaf-lib@^1.0.0
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

        std::fs::remove_dir_all(cwd.join("leaf-lib-v1-0")).unwrap();
        std::fs::remove_dir_all(cwd.join("leaf-lib-v1-5")).unwrap();
        std::fs::remove_dir_all(cwd.join("mid-lib")).unwrap();

        // Add mid-lib with caret requirement to pkg1
        crate::cli!()
            .args(["add", "--registry", url, "test-repo/mid-lib@^2.0.0"])
            .env("BUFFRS_HOME", &buffrs_home)
            .current_dir(cwd.join("pkg1"))
            .assert()
            .success();

        // Install at workspace root
        crate::cli!()
            .arg("install")
            .env("BUFFRS_HOME", &buffrs_home)
            .current_dir(&cwd)
            .assert()
            .success();

        let lockfile_path = cwd.join("Proto.lock");
        assert!(lockfile_path.exists(), "Proto.lock should exist");

        let lockfile = std::fs::read_to_string(&lockfile_path).unwrap();

        // mid-lib@^2.0.0 should resolve to the only available: 2.0.0
        assert!(
            lockfile.contains("name = \"mid-lib\""),
            "lockfile should contain mid-lib"
        );
        assert!(
            lockfile.contains("version = \"2.0.0\""),
            "mid-lib@^2.0.0 should resolve to 2.0.0, got:\n{}",
            lockfile
        );

        // mid-lib's transitive dep leaf-lib@^1.0.0 should resolve to 1.5.0
        assert!(
            lockfile.contains("name = \"leaf-lib\""),
            "lockfile should contain transitive dependency leaf-lib"
        );
        assert!(
            lockfile.contains("version = \"1.5.0\""),
            "leaf-lib@^1.0.0 should resolve to 1.5.0 (highest compatible), got:\n{}",
            lockfile
        );
        assert!(
            !lockfile.contains("version = \"1.0.0\""),
            "leaf-lib must not pin to the minimum 1.0.0 when 1.5.0 is available"
        );
    })
}
