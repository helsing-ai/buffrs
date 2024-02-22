{ fetchurl, runCommand, lib, buffrs, symlinkJoin }:
lockfile:

let
  src = runCommand "vendor-lockfile" { } ''
    mkdir -p $out
    cp ${lockfile} $out/Proto.lock
  '';

  fileRequirementsJson =
    runCommand "buffrs-urls" { buildInputs = [ buffrs ]; } ''
      cd ${src}
      buffrs lock print-files > $out
    '';

  fileRequirements = builtins.fromJSON (builtins.readFile fileRequirementsJson);

  cachePackage = (file:
    let
      prefix = "sha256:";
      sha256 = with lib file;
        assert strings.hasPrefix prefix digest;
        strings.removePrefix prefix digest;

      tar = fetchurl {
        inherit sha256;
        url = file.url;
      };
    in runCommand "extract" { } (with file; ''
      mkdir -p $out
      cp ${tar} $out/${package}.sha256.${digest}.tgz
    ''));

  cache = map cachePackage fileRequirements;
in {
  BUFFRS_CACHE_DIR = symlinkJoin {
    name = "buffrs-cache";
    paths = cache;
  };
}
