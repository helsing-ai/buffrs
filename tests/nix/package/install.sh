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
