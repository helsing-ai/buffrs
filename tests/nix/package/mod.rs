use crate::{VirtualFileSystem, with_test_registry};

const INSTALL_SCRIPT: &str = r#"
set -euo pipefail

# --- verify BUFFRS_CACHE was populated ---
if [ -z "''${BUFFRS_CACHE:-}" ]; then
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
cp ${./Proto.toml} $workdir/Proto.toml
cp ${./Proto.lock} $workdir/Proto.lock
cp -r ${./proto} $workdir/proto
chmod -R u+w $workdir
cd $workdir

export BUFFRS_HOME=$(mktemp -d)

buffrs install

# --- verify installation results ---
test -d proto/vendor
test -d proto/vendor/remote-lib
test -f proto/vendor/remote-lib/remote.proto

grep -q "package remote" proto/vendor/remote-lib/remote.proto

mkdir -p $out
cp -r proto/vendor $out/vendor
"#;

// NOTE: Requires nix and git
#[test]
#[ignore]
fn fixture() {
    with_test_registry(|url| {
        let vfs = VirtualFileSystem::copy(crate::parent_directory!().join("in"));
        let buffrs_home = vfs.root().join("$HOME");
        let cwd = vfs.root();

        // 1. Create and publish remote-lib to the test registry
        super::publish_lib(
            &cwd,
            &buffrs_home,
            url,
            "remote-lib",
            "remote.proto",
            "syntax = \"proto3\";\n\npackage remote;\n\nmessage Data {\n  string value = 1;\n}\n",
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
