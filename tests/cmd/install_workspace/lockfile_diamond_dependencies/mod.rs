use crate::{VirtualFileSystem, with_test_registry};

#[test]
fn fixture() {
    with_test_registry(|url| {
        let vfs = VirtualFileSystem::copy(crate::parent_directory!().join("in"));
        let buffrs_home = vfs.root().join("$HOME");
        let cwd = vfs.root();

        // Create and publish lib-b (leaf package)
        {
            std::fs::create_dir(cwd.join("lib-b")).unwrap();
            let lib_dir = cwd.join("lib-b");

            crate::cli!()
                .args(["init", "--lib", "lib-b"])
                .env("BUFFRS_HOME", &buffrs_home)
                .current_dir(&lib_dir)
                .assert()
                .success();

            std::fs::write(
                lib_dir.join("proto/libb.proto"),
                "syntax = \"proto3\";\n\npackage libb;\n\nmessage LibBMessage {\n  string value = 1;\n}\n",
            )
            .unwrap();

            crate::cli!()
                .args(["publish", "--registry", url, "--repository", "test-repo"])
                .env("BUFFRS_HOME", &buffrs_home)
                .current_dir(&lib_dir)
                .assert()
                .success();
        }

        // Create and publish lib-a (depends on lib-b)
        {
            std::fs::create_dir(cwd.join("lib-a")).unwrap();
            let lib_dir = cwd.join("lib-a");

            crate::cli!()
                .args(["init", "--lib", "lib-a"])
                .env("BUFFRS_HOME", &buffrs_home)
                .current_dir(&lib_dir)
                .assert()
                .success();

            std::fs::write(
                lib_dir.join("proto/liba.proto"),
                "syntax = \"proto3\";\n\npackage liba;\n\nmessage LibAMessage {\n  string value = 1;\n}\n",
            )
            .unwrap();

            // Add lib-b as dependency
            crate::cli!()
                .args(["add", "--registry", url, "test-repo/lib-b@=1.0.0"])
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

        // Add lib-a to pkg1
        crate::cli!()
            .args(["add", "--registry", url, "test-repo/lib-a@=1.0.0"])
            .env("BUFFRS_HOME", &buffrs_home)
            .current_dir(&cwd.join("pkg1"))
            .assert()
            .success();

        // Add lib-a to pkg2
        crate::cli!()
            .args(["add", "--registry", url, "test-repo/lib-a@=1.0.0"])
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

        // Prepare expected output with dynamic values
        let lockfile_path = cwd.join("Proto.lock");
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
        for file in [
            "pkg1/Proto.toml",
            "pkg1/proto/vendor/lib-a/Proto.toml",
            "pkg2/Proto.toml",
            "pkg2/proto/vendor/lib-a/Proto.toml",
            "Proto.lock",
        ] {
            let path = temp_expected.join(file);
            if path.exists() {
                let content = std::fs::read_to_string(&path).unwrap();
                let updated = content.replace("REGISTRY_URL", url);
                std::fs::write(&path, updated).unwrap();
            }
        }

        // Replace DIGEST placeholders in expected lockfile
        let actual_lockfile = std::fs::read_to_string(&lockfile_path).unwrap();

        // Extract digests for both packages
        let digest_a = actual_lockfile
            .split("[[packages]]")
            .find(|s| s.contains("name = \"lib-a\""))
            .and_then(|s| s.lines().find(|line| line.starts_with("digest = ")))
            .unwrap()
            .trim();

        let digest_b = actual_lockfile
            .split("[[packages]]")
            .find(|s| s.contains("name = \"lib-b\""))
            .and_then(|s| s.lines().find(|line| line.starts_with("digest = ")))
            .unwrap()
            .trim();

        let expected_lockfile_path = temp_expected.join("Proto.lock");
        if expected_lockfile_path.exists() {
            let content = std::fs::read_to_string(&expected_lockfile_path).unwrap();
            let updated = content.replace("digest = \"DIGEST_A\"", digest_a);
            let updated = updated.replace("digest = \"DIGEST_B\"", digest_b);
            std::fs::write(&expected_lockfile_path, updated).unwrap();
        }

        vfs.verify_against(temp_expected);
    })
}
