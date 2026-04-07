use crate::{VirtualFileSystem, with_test_registry};

/// Verifies that a caret version requirement (^1.0.0) resolves to the highest
/// compatible version available in the registry, not the minimum.
///
/// Publishes leaf-lib at v1.0.0, v1.1.0, and v1.2.0, then installs with
/// `^1.0.0`. Expects the lockfile to pin v1.2.0.
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

        // Publish leaf-lib at v1.1.0
        {
            let lib_dir = cwd.join("leaf-lib-v1-1");
            std::fs::create_dir(&lib_dir).unwrap();

            crate::cli!()
                .args(["init", "--lib", "leaf-lib"])
                .env("BUFFRS_HOME", &buffrs_home)
                .current_dir(&lib_dir)
                .assert()
                .success();

            let manifest_path = lib_dir.join("Proto.toml");
            let manifest = std::fs::read_to_string(&manifest_path).unwrap();
            let updated = manifest.replace("version = \"0.1.0\"", "version = \"1.1.0\"");
            std::fs::write(&manifest_path, updated).unwrap();

            std::fs::write(
                lib_dir.join("proto/leaf.proto"),
                "syntax = \"proto3\";\n\npackage leaf;\n\nmessage LeafMessage {\n  string value = 1;\n  string extra = 2;\n}\n",
            )
            .unwrap();

            crate::cli!()
                .args(["publish", "--registry", url, "--repository", "test-repo"])
                .env("BUFFRS_HOME", &buffrs_home)
                .current_dir(&lib_dir)
                .assert()
                .success();
        }

        // Publish leaf-lib at v1.2.0 (should be the resolved version)
        {
            let lib_dir = cwd.join("leaf-lib-v1-2");
            std::fs::create_dir(&lib_dir).unwrap();

            crate::cli!()
                .args(["init", "--lib", "leaf-lib"])
                .env("BUFFRS_HOME", &buffrs_home)
                .current_dir(&lib_dir)
                .assert()
                .success();

            let manifest_path = lib_dir.join("Proto.toml");
            let manifest = std::fs::read_to_string(&manifest_path).unwrap();
            let updated = manifest.replace("version = \"0.1.0\"", "version = \"1.2.0\"");
            std::fs::write(&manifest_path, updated).unwrap();

            std::fs::write(
                lib_dir.join("proto/leaf.proto"),
                "syntax = \"proto3\";\n\npackage leaf;\n\nmessage LeafMessage {\n  string value = 1;\n  string extra = 2;\n  int32 count = 3;\n}\n",
            )
            .unwrap();

            crate::cli!()
                .args(["publish", "--registry", url, "--repository", "test-repo"])
                .env("BUFFRS_HOME", &buffrs_home)
                .current_dir(&lib_dir)
                .assert()
                .success();
        }

        std::fs::remove_dir_all(cwd.join("leaf-lib-v1")).unwrap();
        std::fs::remove_dir_all(cwd.join("leaf-lib-v1-1")).unwrap();
        std::fs::remove_dir_all(cwd.join("leaf-lib-v1-2")).unwrap();

        // Add leaf-lib with caret requirement to pkg1
        crate::cli!()
            .args(["add", "--registry", url, "test-repo/leaf-lib@^1.0.0"])
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

        // ^1.0.0 must resolve to the highest 1.x.y: 1.2.0
        assert!(
            lockfile.contains("version = \"1.2.0\""),
            "^1.0.0 should resolve to 1.2.0 (highest compatible), got:\n{}",
            lockfile
        );
        assert!(
            !lockfile.contains("version = \"1.0.0\""),
            "^1.0.0 must not pin to the minimum 1.0.0"
        );
        assert!(
            !lockfile.contains("version = \"1.1.0\""),
            "^1.0.0 must not pin to the intermediate 1.1.0"
        );
    })
}
