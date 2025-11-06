use crate::{VirtualFileSystem, with_test_registry};

#[test]
fn fixture() {
    with_test_registry(|url| {
        let vfs = VirtualFileSystem::copy(crate::parent_directory!().join("in"));
        let buffrs_home = vfs.root().join("$HOME");
        let cwd = vfs.root();

        // Create and publish remote-lib to the test registry
        {
            std::fs::create_dir(cwd.join("remote-lib")).unwrap();
            let lib_dir = cwd.join("remote-lib");

            crate::cli!()
                .args(["init", "--lib", "remote-lib"])
                .env("BUFFRS_HOME", &buffrs_home)
                .current_dir(&lib_dir)
                .assert()
                .success();

            std::fs::write(
                lib_dir.join("proto/remote.proto"),
                "syntax = \"proto3\";\n\npackage remote;\n\nmessage Data {\n  string value = 1;\n}\n",
            )
            .unwrap();

            crate::cli!()
                .args(["publish", "--registry", url, "--repository", "test-repo"])
                .env("BUFFRS_HOME", &buffrs_home)
                .current_dir(&lib_dir)
                .assert()
                .success();
        }

        // Add remote-lib to pkg1
        crate::cli!()
            .args(["add", "--registry", url, "test-repo/remote-lib@=0.1.0"])
            .env("BUFFRS_HOME", &buffrs_home)
            .current_dir(&cwd.join("pkg1"))
            .assert()
            .success();

        // Run install
        crate::cli!()
            .arg("install")
            .env("BUFFRS_HOME", &buffrs_home)
            .current_dir(&cwd)
            .assert()
            .success();

        // Verify lockfile was created at workspace root
        let lockfile_path = cwd.join("Proto.lock");
        assert!(lockfile_path.exists(), "Proto.lock should exist at workspace root");

        // Replace REGISTRY_URL and DIGEST placeholders in expected output
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

        // Update registry URL in expected files
        for file in ["pkg1/Proto.toml", "pkg2/proto/vendor/workspace-pkg1/Proto.toml", "Proto.lock"] {
            let path = temp_expected.join(file);
            if path.exists() {
                let content = std::fs::read_to_string(&path).unwrap();
                let updated = content.replace("REGISTRY_URL", url);
                std::fs::write(&path, updated).unwrap();
            }
        }

        // Update digest in expected lockfile
        let actual_lockfile_content = std::fs::read_to_string(&lockfile_path).unwrap();
        let digest = actual_lockfile_content
            .lines()
            .find(|line| line.starts_with("digest = "))
            .unwrap()
            .trim();

        let expected_lockfile_path = temp_expected.join("Proto.lock");
        if expected_lockfile_path.exists() {
            let content = std::fs::read_to_string(&expected_lockfile_path).unwrap();
            let updated = content.replace("digest = \"DIGEST\"", digest);
            std::fs::write(&expected_lockfile_path, updated).unwrap();
        }

        vfs.verify_against(temp_expected);
    })
}
