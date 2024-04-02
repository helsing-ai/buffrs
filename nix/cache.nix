{ fetchurl, runCommand, lib, buffrs, symlinkJoin }:

lockfile:

let
  lock = builtins.fromTOML (builtins.readFile lockfile);

  intoFileRequirement = locked: {
    inherit (locked) digest;
    url = locked.registry + "/" + locked.repository + "/" + locked.name + "/" locked.name + "-" +  locked.version + ".tgz";
  };

  fileRequirements = map intoFileRequirement lock.packages;

  cachePackage = (file:
    let
      prefix = "sha256:";

      sha256 = assert lib.strings.hasPrefix prefix file.digest;
        lib.strings.removePrefix prefix file.digest;

      tar = fetchurl {
        inherit sha256;
        url = file.url;
      };
    in runCommand "cache-${file.package}" { } ''
      mkdir -p $out
      cp ${tar} $out/${file.package}.sha256.${sha256}.tgz
    '');

  cache = map cachePackage fileRequirements;
in {
  BUFFRS_CACHE = symlinkJoin {
    name = "buffrs-cache";
    paths = cache;
  };
}
