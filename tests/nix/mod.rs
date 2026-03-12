use crate::{VirtualFileSystem, with_test_registry};
use std::process::Command;

/// Generate a flake.nix that verifies the full nix integration:
///
/// 1. `vendorDependencies` computes BUFFRS_CACHE (downloads via fetchurl — fixed-output derivation)
/// 2. A sandboxed derivation (no network) runs `buffrs install` using only the cache
/// 3. The derivation verifies the vendor directory contains the expected protos
/// 4. A `check` references the package so `nix flake check` exercises the full flow
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

        installed = pkgs.runCommand "buffrs-nix-install" ({{
          buildInputs = [ buffrs-bin ];
        }} // vendored) ''
          set -euo pipefail

          # --- verify BUFFRS_CACHE was populated ---
          if [ -z "''${{BUFFRS_CACHE:-}}" ]; then
            echo "BUFFRS_CACHE is not set" >&2
            exit 1
          fi

          cache_files=$(ls "$BUFFRS_CACHE"/*.tgz 2>/dev/null || true)
          if [ -z "$cache_files" ]; then
            echo "BUFFRS_CACHE is empty — no .tgz files found" >&2
            exit 1
          fi

          echo "BUFFRS_CACHE contents:"
          ls -la "$BUFFRS_CACHE"

          found_remote_lib=false
          for f in "$BUFFRS_CACHE"/*.tgz; do
            basename=$(basename "$f")
            case "$basename" in
              remote-lib.*) found_remote_lib=true ;;
            esac
          done

          if [ "$found_remote_lib" != "true" ]; then
            echo "BUFFRS_CACHE does not contain remote-lib" >&2
            exit 1
          fi

          # --- run buffrs install in a sandbox (no network) ---
          workdir=$(mktemp -d)
          cp ${{./Proto.toml}} $workdir/Proto.toml
          cp ${{./Proto.lock}} $workdir/Proto.lock
          cp -r ${{./proto}} $workdir/proto
          chmod -R u+w $workdir
          cd $workdir

          export BUFFRS_HOME=$(mktemp -d)

          buffrs install

          # --- verify installation results ---
          test -d proto/vendor
          test -d proto/vendor/remote-lib
          test -f proto/vendor/remote-lib/remote.proto

          # verify the proto content is correct
          grep -q "package remote" proto/vendor/remote-lib/remote.proto

          mkdir -p $out
          cp -r proto/vendor $out/vendor
        '';
      in {{
        packages.default = installed;
        checks.default = installed;
      }}
    );
}}"#
    )
}

// NOTE: Requires nix and git
#[test]
#[ignore]
fn fixture() {
    with_test_registry(|url| {
        let vfs = VirtualFileSystem::copy(crate::parent_directory!().join("in"));
        let buffrs_home = vfs.root().join("$HOME");
        let cwd = vfs.root();

        // 1. Create and publish remote-lib to the test registry
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

        // 4.1 Copy clean proto sources (without vendor/) from fixtures
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

        // 4.2 Write the flake.nix
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

        // 5. Build the package and verify the output contains the vendored proto
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
            result_path.join("vendor/remote-lib/remote.proto").exists(),
            "derivation output missing vendor/remote-lib/remote.proto"
        );

        // 6. Run nix flake check to exercise the check derivation
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
