use crate::{VirtualFileSystem, with_test_registry};

#[test]
fn fixture() {
    with_test_registry(|url| {
        let vfs = VirtualFileSystem::copy(crate::parent_directory!().join("in"));
        let buffrs_home = vfs.root().join("$HOME");
        let cwd = vfs.root();

        // Create and publish remote-lib v1.0.0
        {
            std::fs::create_dir(cwd.join("remote-lib-v1")).unwrap();
            let lib_dir = cwd.join("remote-lib-v1");

            crate::cli!()
                .args(["init", "--lib", "remote-lib"])
                .env("BUFFRS_HOME", &buffrs_home)
                .current_dir(&lib_dir)
                .assert()
                .success();

            // Update version to 1.0.0
            let manifest_path = lib_dir.join("Proto.toml");
            let manifest = std::fs::read_to_string(&manifest_path).unwrap();
            let updated = manifest.replace("version = \"0.1.0\"", "version = \"1.0.0\"");
            std::fs::write(&manifest_path, updated).unwrap();

            std::fs::write(
                lib_dir.join("proto/remote.proto"),
                "syntax = \"proto3\";\n\npackage remote.v1;\n\nmessage DataV1 {\n  string value = 1;\n}\n",
            )
            .unwrap();

            crate::cli!()
                .args(["publish", "--registry", url, "--repository", "test-repo"])
                .env("BUFFRS_HOME", &buffrs_home)
                .current_dir(&lib_dir)
                .assert()
                .success();
        }

        // Create and publish remote-lib v2.0.0
        {
            std::fs::create_dir(cwd.join("remote-lib-v2")).unwrap();
            let lib_dir = cwd.join("remote-lib-v2");

            crate::cli!()
                .args(["init", "--lib", "remote-lib"])
                .env("BUFFRS_HOME", &buffrs_home)
                .current_dir(&lib_dir)
                .assert()
                .success();

            // Update version to 2.0.0
            let manifest_path = lib_dir.join("Proto.toml");
            let manifest = std::fs::read_to_string(&manifest_path).unwrap();
            let updated = manifest.replace("version = \"0.1.0\"", "version = \"2.0.0\"");
            std::fs::write(&manifest_path, updated).unwrap();

            std::fs::write(
                lib_dir.join("proto/remote.proto"),
                "syntax = \"proto3\";\n\npackage remote.v2;\n\nmessage DataV2 {\n  string value = 1;\n  int32 count = 2;\n}\n",
            )
            .unwrap();

            crate::cli!()
                .args(["publish", "--registry", url, "--repository", "test-repo"])
                .env("BUFFRS_HOME", &buffrs_home)
                .current_dir(&lib_dir)
                .assert()
                .success();
        }

        // Add remote-lib@=1.0.0 to pkg1
        crate::cli!()
            .args(["add", "--registry", url, "test-repo/remote-lib@=1.0.0"])
            .env("BUFFRS_HOME", &buffrs_home)
            .current_dir(&cwd.join("pkg1"))
            .assert()
            .success();

        // Add remote-lib@=2.0.0 to pkg2
        crate::cli!()
            .args(["add", "--registry", url, "test-repo/remote-lib@=2.0.0"])
            .env("BUFFRS_HOME", &buffrs_home)
            .current_dir(&cwd.join("pkg2"))
            .assert()
            .success();

        // Run install at workspace root
        crate::cli!()
            .arg("install")
            .env("BUFFRS_HOME", &buffrs_home)
            .current_dir(&cwd)
            .assert()
            .success();

        // Verify workspace lockfile exists
        let lockfile_path = cwd.join("Proto.lock");
        assert!(lockfile_path.exists(), "Proto.lock should exist at workspace root");

        // Verify lockfile contains both versions
        let lockfile_content = std::fs::read_to_string(&lockfile_path).unwrap();
        assert!(lockfile_content.contains("version = \"1.0.0\""), "Lockfile should have v1.0.0");
        assert!(lockfile_content.contains("version = \"2.0.0\""), "Lockfile should have v2.0.0");

        // Prepare expected output with dynamic values
        let out_dir = crate::parent_directory!().join("out");
        let vfs_root = vfs.root();
        let temp_root = vfs_root.parent().unwrap();
        let temp_expected = temp_root.join("expected");

        fs_extra::dir::copy(
            &out_dir,
            &temp_expected,
            &fs_extra::dir::CopyOptions {
                overwrite: true,
                skip_exist: false,
                buffer_size: 8192,
                copy_inside: false,
                content_only: true,
                depth: 64,
            },
        )
        .unwrap();

        // Replace REGISTRY_URL in expected files
        for file in ["pkg1/Proto.toml", "pkg2/Proto.toml", "Proto.lock"] {
            let path = temp_expected.join(file);
            if path.exists() {
                let content = std::fs::read_to_string(&path).unwrap();
                let updated = content.replace("REGISTRY_URL", url);
                std::fs::write(&path, updated).unwrap();
            }
        }

        // Replace DIGEST placeholders in expected lockfile
        let actual_lockfile = std::fs::read_to_string(&lockfile_path).unwrap();

        // Extract digests for both versions
        let digest_v1 = actual_lockfile
            .lines()
            .skip_while(|line| !line.contains("version = \"1.0.0\""))
            .find(|line| line.starts_with("digest = "))
            .unwrap()
            .trim();

        let digest_v2 = actual_lockfile
            .lines()
            .skip_while(|line| !line.contains("version = \"2.0.0\""))
            .find(|line| line.starts_with("digest = "))
            .unwrap()
            .trim();

        let expected_lockfile_path = temp_expected.join("Proto.lock");
        if expected_lockfile_path.exists() {
            let content = std::fs::read_to_string(&expected_lockfile_path).unwrap();
            // Replace first occurrence with v1 digest, second with v2 digest
            let updated = content.replacen("digest = \"DIGEST_V1\"", digest_v1, 1);
            let updated = updated.replacen("digest = \"DIGEST_V2\"", digest_v2, 1);
            std::fs::write(&expected_lockfile_path, updated).unwrap();
        }

        vfs.verify_against(temp_expected);
    })
}
