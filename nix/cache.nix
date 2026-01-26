{
  fetchurl,
  runCommand,
  lib,
  buffrs,
  symlinkJoin,
}:

lockfile:

let
  src = runCommand "vendor-lockfile" { } ''
    mkdir -p $out
    cp ${lockfile} $out/Proto.lock
  '';

  fileRequirementsJson = runCommand "buffrs-urls" { buildInputs = [ buffrs ]; } ''
    cd ${src}
    buffrs lock print-files
    buffrs lock print-files > $out
  '';

  fileRequirements =
    let
      parsed = builtins.fromJSON (builtins.readFile fileRequirementsJson);
    in
    builtins.trace parsed parsed;

  cachePackage = (
    file:
    let
      prefix = "sha256:";

      sha256 =
        assert lib.strings.hasPrefix prefix file.digest;
        lib.strings.removePrefix prefix file.digest;

      tar = fetchurl {
        inherit sha256;
        url = file.url;
      };
    in
    runCommand "cache-${file.package}" { } ''
      mkdir -p $out
      cp ${tar} $out/${file.package}.sha256.${sha256}.tgz
    ''
  );

  cache = map cachePackage fileRequirements;
in
{
  BUFFRS_CACHE = symlinkJoin {
    name = "buffrs-cache";
    paths = cache;
  };
}
