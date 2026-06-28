use crate::{VirtualFileSystem, with_test_registry};

/// Verifies that compatible but different range requirements on the same
/// transitive leaf are resolved to a single version within one package's
/// dependency graph.
///
/// Diamond shape (all within pkg1):
///   pkg1 --(^1.0)--> lib-a --(^1.0.0)--> leaf-lib
///   pkg1 --(^1.0)--> lib-b --(>=1.2.0)--> leaf-lib
///
/// leaf-lib is available at v1.0.0 and v1.5.0. The resolver encounters
/// leaf-lib first via lib-a (resolves to v1.5.0 for ^1.0.0), then
/// re-encounters it via lib-b and calls validate_version_compatibility,
/// which should accept v1.5.0 because it satisfies >=1.2.0. Install must
/// succeed and the lockfile must pin leaf-lib at v1.5.0 exactly once.
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

        // Publish leaf-lib at v1.5.0 (satisfies both ^1.0.0 and >=1.2.0)
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

        // Publish lib-a at v1.0.0 (depends on leaf-lib@^1.0.0)
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

        // Publish lib-b at v1.0.0 (depends on leaf-lib@>=1.2.0, compatible with 1.5.0)
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
                .args(["add", "--registry", url, "test-repo/leaf-lib@>=1.2.0"])
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
        std::fs::remove_dir_all(cwd.join("lib-a")).unwrap();
        std::fs::remove_dir_all(cwd.join("lib-b")).unwrap();

        // pkg1 depends on BOTH lib-a and lib-b — creating a diamond on leaf-lib
        // within a single package's dependency graph.
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

        // Install must succeed — 1.5.0 satisfies both ^1.0.0 and >=1.2.0
        crate::cli!()
            .arg("install")
            .env("BUFFRS_HOME", &buffrs_home)
            .current_dir(&cwd)
            .assert()
            .success();

        let lockfile_path = cwd.join("Proto.lock");
        assert!(lockfile_path.exists(), "Proto.lock should exist");

        let lockfile = std::fs::read_to_string(&lockfile_path).unwrap();

        // leaf-lib must appear exactly once in the lockfile
        let leaf_count = lockfile.matches("name = \"leaf-lib\"").count();
        assert_eq!(
            leaf_count, 1,
            "leaf-lib should appear exactly once in the lockfile (got {})",
            leaf_count
        );

        // Find the leaf-lib section and verify its pinned version
        let leaf_section = lockfile
            .split("[[packages]]")
            .find(|s| s.contains("name = \"leaf-lib\""))
            .expect("leaf-lib section should exist in lockfile");

        assert!(
            leaf_section.contains("version = \"1.5.0\""),
            "leaf-lib should be pinned at 1.5.0 (satisfies both ^1.0.0 and >=1.2.0), got:\n{}",
            leaf_section
        );
        assert!(
            !leaf_section.contains("version = \"1.0.0\""),
            "leaf-lib must not be pinned at 1.0.0 (does not satisfy >=1.2.0), got:\n{}",
            leaf_section
        );
    })
}
