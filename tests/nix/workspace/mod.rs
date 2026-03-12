use crate::{VirtualFileSystem, with_test_registry};

const INSTALL_SCRIPT: &str = r#"
set -euo pipefail

# --- verify BUFFRS_CACHE contains all remote packages ---
echo "BUFFRS_CACHE contents:"
ls -la "$BUFFRS_CACHE"

for pkg in remote-lib-a remote-lib-b pkg-c; do
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

cp ${./Proto.toml} $workdir/Proto.toml
cp ${./Proto.lock} $workdir/Proto.lock

for member in pkg-a pkg-b; do
  mkdir -p $workdir/$member
  cp ${./.}/$member/Proto.toml $workdir/$member/Proto.toml
  cp -r ${./.}/$member/proto $workdir/$member/proto
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

# --- verify pkg-b has pkg-c (remote) but not remote-lib-a/b ---
test -f pkg-b/proto/vendor/pkg-c/c.proto
grep -q "package pkg.c" pkg-b/proto/vendor/pkg-c/c.proto

if [ -d pkg-b/proto/vendor/remote-lib-a ] || [ -d pkg-b/proto/vendor/remote-lib-b ]; then
  echo "pkg-b should not have remote-lib-a or remote-lib-b" >&2
  exit 1
fi

mkdir -p $out
cp -r pkg-a/proto/vendor $out/pkg-a-vendor
cp -r pkg-b/proto/vendor $out/pkg-b-vendor
"#;

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
            "syntax = \"proto3\";\n\npackage pkg.c;\n\nmessage MessageC {\n  string value = 1;\n}\n",
        );

        // 2. Publish remote-lib-b (leaf dependency of remote-lib-a)
        super::publish::lib(
            &cwd,
            &buffrs_home,
            url,
            "remote-lib-b",
            "b.proto",
            "syntax = \"proto3\";\n\npackage remote.b;\n\nmessage RemoteB {\n  string value = 1;\n}\n",
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
