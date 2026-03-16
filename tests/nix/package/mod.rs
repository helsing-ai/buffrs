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

        // 1. Create and publish remote-lib to the test registry
        super::publish::lib(
            &cwd,
            &buffrs_home,
            url,
            "remote-lib",
            "remote.proto",
            r#"
              syntax = "proto3";

              package remote;

              message Data {
                string value = 1;
              }
            "#
            .trim(),
        );

        // 2. Add remote-lib as a dependency
        crate::cli!()
            .args(["add", "--registry", url, "test-repo/remote-lib@=0.1.0"])
            .env("BUFFRS_HOME", &buffrs_home)
            .current_dir(&cwd)
            .assert()
            .success();

        // 3. Run install to generate Proto.lock
        crate::cli!()
            .arg("install")
            .env("BUFFRS_HOME", &buffrs_home)
            .current_dir(&cwd)
            .assert()
            .success();

        // 4. Set up a nix flake directory
        let nix_dir = tempfile::TempDir::new().unwrap();
        let nix_path = nix_dir.path();

        std::fs::copy(cwd.join("Proto.toml"), nix_path.join("Proto.toml")).unwrap();
        std::fs::copy(cwd.join("Proto.lock"), nix_path.join("Proto.lock")).unwrap();

        fs_extra::dir::copy(
            crate::parent_directory!().join("in/proto"),
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

        // 5. Build + check via nix
        let mut flake = super::Flake::builder()
            .repo(env!("CARGO_MANIFEST_DIR"))
            .name("buffrs-nix-install")
            .script(INSTALL_SCRIPT)
            .build();

        flake.write(nix_path);

        let result = flake.build();

        assert!(
            result.join("vendor/remote-lib/remote.proto").exists(),
            "derivation output missing vendor/remote-lib/remote.proto"
        );

        flake.check();
    });
}
