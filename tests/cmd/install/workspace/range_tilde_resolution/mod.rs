use crate::{VirtualFileSystem, with_test_registry};

/// Verifies that a tilde version requirement (~1.2.0) resolves to the highest
/// patch version within the same minor, and does not cross into the next minor.
///
/// Publishes leaf-lib at v1.2.0, v1.2.5, and v1.3.0, then installs with
/// `~1.2.0`. Expects the lockfile to pin v1.2.5 (not v1.3.0).
#[test]
fn fixture() {
    with_test_registry(|url| {
        let vfs = VirtualFileSystem::copy(crate::parent_directory!().join("in"));
        let buffrs_home = vfs.root().join("$HOME");
        let cwd = vfs.root();

        // Publish leaf-lib at v1.2.0
        {
            let lib_dir = cwd.join("leaf-lib-v1-2-0");
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

        // Publish leaf-lib at v1.2.5 (highest in the 1.2.x range — should be selected)
        {
            let lib_dir = cwd.join("leaf-lib-v1-2-5");
            std::fs::create_dir(&lib_dir).unwrap();

            crate::cli!()
                .args(["init", "--lib", "leaf-lib"])
                .env("BUFFRS_HOME", &buffrs_home)
                .current_dir(&lib_dir)
                .assert()
                .success();

            let manifest_path = lib_dir.join("Proto.toml");
            let manifest = std::fs::read_to_string(&manifest_path).unwrap();
            let updated = manifest.replace("version = \"0.1.0\"", "version = \"1.2.5\"");
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

        // Publish leaf-lib at v1.3.0 (out of ~1.2.x range — must NOT be selected)
        {
            let lib_dir = cwd.join("leaf-lib-v1-3-0");
            std::fs::create_dir(&lib_dir).unwrap();

            crate::cli!()
                .args(["init", "--lib", "leaf-lib"])
                .env("BUFFRS_HOME", &buffrs_home)
                .current_dir(&lib_dir)
                .assert()
                .success();

            let manifest_path = lib_dir.join("Proto.toml");
            let manifest = std::fs::read_to_string(&manifest_path).unwrap();
            let updated = manifest.replace("version = \"0.1.0\"", "version = \"1.3.0\"");
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

        std::fs::remove_dir_all(cwd.join("leaf-lib-v1-2-0")).unwrap();
        std::fs::remove_dir_all(cwd.join("leaf-lib-v1-2-5")).unwrap();
        std::fs::remove_dir_all(cwd.join("leaf-lib-v1-3-0")).unwrap();

        // Add leaf-lib with tilde requirement to pkg1
        crate::cli!()
            .args(["add", "--registry", url, "test-repo/leaf-lib@~1.2.0"])
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

        // ~1.2.0 must resolve to the highest 1.2.x: 1.2.5
        assert!(
            lockfile.contains("version = \"1.2.5\""),
            "~1.2.0 should resolve to 1.2.5 (highest patch in 1.2.x), got:\n{}",
            lockfile
        );
        assert!(
            !lockfile.contains("version = \"1.3.0\""),
            "~1.2.0 must not cross into the next minor (1.3.0)"
        );
        assert!(
            !lockfile.contains("version = \"1.2.0\""),
            "~1.2.0 must not pin to the minimum patch 1.2.0"
        );
    })
}
