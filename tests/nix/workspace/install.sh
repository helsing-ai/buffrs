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
