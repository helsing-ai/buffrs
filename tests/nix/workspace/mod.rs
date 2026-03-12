use crate::{VirtualFileSystem, with_test_registry};
use std::process::Command;

/// Generate a flake.nix for a workspace with mixed local + remote transitive dependencies.
///
/// The nix derivation:
/// 1. Verifies BUFFRS_CACHE contains both remote packages (remote-lib-a + remote-lib-b)
/// 2. Runs `buffrs install` in a sandbox (no network) using only the cache
/// 3. Verifies all vendor directories are populated correctly
fn generate_flake_nix(buffrs_repo: &str) -> String {
    format!(
        r#"{{
  inputs = {{
    buffrs.url = "path:{buffrs_repo}";
    nixpkgs.follows = "buffrs/nixpkgs";
    flake-utils.follows = "buffrs/flake-utils";
  }};

  outputs = {{ buffrs, nixpkgs, flake-utils, ... }}:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs {{ inherit system; }};
        vendored = buffrs.lib.${{system}}.vendorDependencies ./Proto.lock;
        buffrs-bin = buffrs.packages.${{system}}.default;

        installed = pkgs.runCommand "buffrs-nix-workspace-install" ({{
          buildInputs = [ buffrs-bin ];
        }} // vendored) ''
          set -euo pipefail

          # --- verify BUFFRS_CACHE contains both remote packages ---
          echo "BUFFRS_CACHE contents:"
          ls -la "$BUFFRS_CACHE"

          for pkg in remote-lib-a remote-lib-b; do
            found=false
            for f in "$BUFFRS_CACHE"/*.tgz; do
              basename=$(basename "$f")
              case "$basename" in
                "$pkg".*) found=true ;;
              esac
            done
            if [ "$found" != "true" ]; then
              echo "BUFFRS_CACHE does not contain $pkg" >&2
              exit 1
            fi
          done

          # --- set up workspace and run buffrs install (no network) ---
          workdir=$(mktemp -d)

          cp ${{./Proto.toml}} $workdir/Proto.toml
          cp ${{./Proto.lock}} $workdir/Proto.lock

          for member in pkg-a pkg-b; do
            mkdir -p $workdir/$member
            cp ${{./.}}/$member/Proto.toml $workdir/$member/Proto.toml
            cp -r ${{./.}}/$member/proto $workdir/$member/proto
          done

          chmod -R u+w $workdir
          cd $workdir

          export BUFFRS_HOME=$(mktemp -d)

          buffrs install

          # --- verify pkg-a has remote-lib-a, remote-lib-b (transitive), and pkg-b (local) ---
          test -f pkg-a/proto/vendor/remote-lib-a/a.proto
          test -f pkg-a/proto/vendor/remote-lib-b/b.proto
          test -f pkg-a/proto/vendor/pkg-b/b.proto
          grep -q "package remote.a" pkg-a/proto/vendor/remote-lib-a/a.proto
          grep -q "package remote.b" pkg-a/proto/vendor/remote-lib-b/b.proto

          # --- verify pkg-b has no remote vendors (only local deps) ---
          if [ -d pkg-b/proto/vendor/remote-lib-a ] || [ -d pkg-b/proto/vendor/remote-lib-b ]; then
            echo "pkg-b should not have remote vendors" >&2
            exit 1
          fi

          mkdir -p $out
          cp -r pkg-a/proto/vendor $out/pkg-a-vendor
          cp -r pkg-b/proto $out/pkg-b-proto
        '';
      in {{
        packages.default = installed;
        checks.default = installed;
      }}
    );
}}"#
    )
}

/// Helper to create, init, and publish a library package to the test registry.
fn publish_lib(
    cwd: &std::path::Path,
    buffrs_home: &std::path::Path,
    url: &str,
    name: &str,
    proto_filename: &str,
    proto_content: &str,
) {
    std::fs::create_dir(cwd.join(name)).unwrap();
    let lib_dir = cwd.join(name);

    crate::cli!()
        .args(["init", "--lib", name])
        .env("BUFFRS_HOME", buffrs_home)
        .current_dir(&lib_dir)
        .assert()
        .success();

    std::fs::write(lib_dir.join(format!("proto/{proto_filename}")), proto_content).unwrap();

    crate::cli!()
        .args(["publish", "--registry", url, "--repository", "test-repo"])
        .env("BUFFRS_HOME", buffrs_home)
        .current_dir(&lib_dir)
        .assert()
        .success();
}

// NOTE: Requires nix and git
#[test]
#[ignore]
fn fixture() {
    with_test_registry(|url| {
        let vfs = VirtualFileSystem::copy(crate::parent_directory!().join("in"));
        let buffrs_home = vfs.root().join("$HOME");
        let cwd = vfs.root();

        // 1. Publish remote-lib-b (leaf dependency)
        publish_lib(
            &cwd,
            &buffrs_home,
            url,
            "remote-lib-b",
            "b.proto",
            "syntax = \"proto3\";\n\npackage remote.b;\n\nmessage RemoteB {\n  string value = 1;\n}\n",
        );

        // 2. Publish remote-lib-a (depends on remote-lib-b)
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

        // 3. Add remote-lib-a as a dependency of pkg-a
        crate::cli!()
            .args(["add", "--registry", url, "test-repo/remote-lib-a@=0.1.0"])
            .env("BUFFRS_HOME", &buffrs_home)
            .current_dir(cwd.join("pkg-a"))
            .assert()
            .success();

        // 4. Run workspace install to generate Proto.lock
        crate::cli!()
            .arg("install")
            .env("BUFFRS_HOME", &buffrs_home)
            .current_dir(&cwd)
            .assert()
            .success();

        // 5. Set up a nix flake directory
        let nix_dir = tempfile::TempDir::new().unwrap();
        let nix_path = nix_dir.path();

        // Copy workspace root manifests
        std::fs::copy(cwd.join("Proto.toml"), nix_path.join("Proto.toml")).unwrap();
        std::fs::copy(cwd.join("Proto.lock"), nix_path.join("Proto.lock")).unwrap();

        // Copy clean workspace member sources from fixtures (without vendor/)
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

        // Write the flake.nix
        let buffrs_repo = env!("CARGO_MANIFEST_DIR");
        let flake_nix = generate_flake_nix(buffrs_repo);
        std::fs::write(nix_path.join("flake.nix"), flake_nix).unwrap();

        assert!(
            Command::new("git")
                .args(["init"])
                .current_dir(nix_path)
                .status()
                .expect("git not found")
                .success(),
            "git init failed"
        );

        assert!(
            Command::new("git")
                .args(["add", "."])
                .current_dir(nix_path)
                .status()
                .expect("git not found")
                .success(),
            "git add failed"
        );

        // 6. Build the package and verify output
        let output = Command::new("nix")
            .args(["build", "--print-build-logs"])
            .current_dir(nix_path)
            .output()
            .expect("nix not found — this test requires nix to be installed");

        assert!(
            output.status.success(),
            "nix build failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );

        let result_path = nix_path.join("result");

        assert!(
            result_path.join("pkg-a-vendor/remote-lib-a/a.proto").exists(),
            "derivation output missing pkg-a-vendor/remote-lib-a/a.proto"
        );
        assert!(
            result_path.join("pkg-a-vendor/remote-lib-b/b.proto").exists(),
            "derivation output missing pkg-a-vendor/remote-lib-b/b.proto (transitive dep)"
        );
        assert!(
            result_path.join("pkg-a-vendor/pkg-b/b.proto").exists(),
            "derivation output missing pkg-a-vendor/pkg-b/b.proto (local dep)"
        );

        // 7. Run nix flake check
        let output = Command::new("nix")
            .args(["flake", "check", "--print-build-logs"])
            .current_dir(nix_path)
            .output()
            .expect("nix flake check failed");

        assert!(
            output.status.success(),
            "nix flake check failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
    });
}
