# Pure-Nix variant of `cache.nix` — same `BUFFRS_CACHE` output, but
# the URL list is derived by parsing `Proto.lock` with `fromTOML`
# instead of invoking `buffrs lock print-files` under IFD.
#
# Use this when the caller evaluates `vendorDependencies` for a
# system it can't build (e.g. enumerating `aarch64-darwin` outputs
# from `x86_64-linux` via flake-parts). The IFD variant in
# `cache.nix` would force `buffrs-urls` to build on the foreign
# system; this one avoids that entirely.
#
# URL construction mirrors `FileRequirement::new` in `src/lock.rs`:
#   <registry>/<repository>/<name>/<name>-<version>.tgz
# Keep this in sync if the on-the-wire layout changes.
{ fetchurl, runCommand, lib, symlinkJoin, }:

lockfile:

let
  lock = builtins.fromTOML (builtins.readFile lockfile);
  packages = lock.packages or [ ];

  fileUrl = pkg:
    "${pkg.registry}/${pkg.repository}/${pkg.name}/${pkg.name}-${pkg.version}.tgz";

  cachePackage = pkg:
    let
      prefix = "sha256:";
      sha256 = assert lib.strings.hasPrefix prefix pkg.digest;
        lib.strings.removePrefix prefix pkg.digest;
      tar = fetchurl {
        inherit sha256;
        url = fileUrl pkg;
      };
    in runCommand "cache-${pkg.name}" { } ''
      mkdir -p $out
      cp ${tar} $out/${pkg.name}.sha256.${sha256}.tgz
    '';

  cache = map cachePackage packages;
in {
  BUFFRS_CACHE = symlinkJoin {
    name = "buffrs-cache";
    paths = cache;
  };
}
