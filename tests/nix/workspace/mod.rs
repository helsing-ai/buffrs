use crate::{VirtualFileSystem, with_test_registry};

const INSTALL_SCRIPT: super::NixScript = include_str!("install.sh");

// NOTE: Requires nix and git
#[test]
#[ignore]
fn fixture() {
    with_test_registry(|url| {
        let vfs = VirtualFileSystem::copy(crate::parent_directory!().join("in"));
        let buffrs_home = vfs.root().join("$HOME");
        let cwd = vfs.root();

        // 1. Publish pkg-c (standalone remote lib, dependency of pkg-b)
        super::publish::lib(
            &cwd,
            &buffrs_home,
            url,
            "pkg-c",
            "c.proto",
            r#"
              syntax = "proto3";

              package pkg.c;

              message MessageC {
                string value = 1;
              }
            "#
            .trim(),
        );

        // 2. Publish remote-lib-b (leaf dependency of remote-lib-a)
        super::publish::lib(
            &cwd,
            &buffrs_home,
            url,
            "remote-lib-b",
            "b.proto",
            r#"
              syntax = "proto3";

              package remote.b;

              message RemoteB {
                string value = 1;
              }
            "#
            .trim(),
        );

        // 3. Publish remote-lib-a (depends on remote-lib-b)
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
                lib_dir.join("proto/a.proto"),
                "syntax = \"proto3\";\n\npackage remote.a;\n\nmessage RemoteA {\n  string value = 1;\n}\n",
            )
            .unwrap();

            crate::cli!()
                .args(["add", "--registry", url, "test-repo/remote-lib-b@=0.1.0"])
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

        // 4. Add remote-lib-a as a dependency of pkg-a
        crate::cli!()
            .args(["add", "--registry", url, "test-repo/remote-lib-a@=0.1.0"])
            .env("BUFFRS_HOME", &buffrs_home)
            .current_dir(cwd.join("pkg-a"))
            .assert()
            .success();

        // 5. Add pkg-c as a dependency of pkg-b
        crate::cli!()
            .args(["add", "--registry", url, "test-repo/pkg-c@=0.1.0"])
            .env("BUFFRS_HOME", &buffrs_home)
            .current_dir(cwd.join("pkg-b"))
            .assert()
            .success();

        // 6. Run workspace install to generate Proto.lock
        crate::cli!()
            .arg("install")
            .env("BUFFRS_HOME", &buffrs_home)
            .current_dir(&cwd)
            .assert()
            .success();

        // 7. Set up a nix flake directory
        let nix_dir = tempfile::TempDir::new().unwrap();
        let nix_path = nix_dir.path();

        std::fs::copy(cwd.join("Proto.toml"), nix_path.join("Proto.toml")).unwrap();
        std::fs::copy(cwd.join("Proto.lock"), nix_path.join("Proto.lock")).unwrap();

        for member in ["pkg-a", "pkg-b"] {
            let fixture_member = crate::parent_directory!().join("in").join(member);
            fs_extra::dir::copy(
                &fixture_member,
                nix_path,
                &fs_extra::dir::CopyOptions {
                    overwrite: true,
                    skip_exist: false,
                    buffer_size: 8192,
                    copy_inside: false,
                    content_only: false,
                    depth: 64,
                },
            )
            .unwrap();

            // Overwrite Proto.toml with the version that has dependencies added
            std::fs::copy(
                cwd.join(member).join("Proto.toml"),
                nix_path.join(member).join("Proto.toml"),
            )
            .unwrap();
        }

        // 8. Build + check via nix
        let mut flake = super::Flake::builder()
            .repo(env!("CARGO_MANIFEST_DIR"))
            .name("buffrs-nix-workspace-install")
            .script(INSTALL_SCRIPT)
            .build();

        flake.write(nix_path);

        let result = flake.build();

        assert!(
            result.join("pkg-a-vendor/remote-lib-a/a.proto").exists(),
            "derivation output missing pkg-a-vendor/remote-lib-a/a.proto"
        );
        assert!(
            result.join("pkg-a-vendor/remote-lib-b/b.proto").exists(),
            "derivation output missing pkg-a-vendor/remote-lib-b/b.proto (transitive dep)"
        );
        assert!(
            result.join("pkg-a-vendor/pkg-b/b.proto").exists(),
            "derivation output missing pkg-a-vendor/pkg-b/b.proto (local dep)"
        );
        assert!(
            result.join("pkg-b-vendor/pkg-c/c.proto").exists(),
            "derivation output missing pkg-b-vendor/pkg-c/c.proto (remote dep of pkg-b)"
        );

        flake.check();
    });
}
