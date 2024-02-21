{ fetchurl, runCommand, lib, buffrs, symlinkJoin }: lockfile:

let
  src = runCommand "vendor-lockfile" {} ''
    mkdir -p $out
    cp ${lockfile} $out/Proto.lock
  '';

  fileRequirementsJson =
    runCommand "buffrs-urls" { buildInputs = [ buffrs ]; } ''
      cd ${src}
      buffrs lock print-files > $out
    '';

  fileRequirements = builtins.fromJSON (builtins.readFile fileRequirementsJson);

  fetchPackages = (file: let
    prefix = "sha256:";
    sha256 = with lib; with file;
      assert strings.hasPrefix prefix digest;
      strings.removePrefix prefix digest;

    tar = fetchurl {
      inherit sha256;
      url = file.url;
    };
  in runCommand "extract" {} ''
    mkdir -p $out
    cp ${tar} $out/sha256-${tar}.tgz
  '');

  allPackages = map fetchPackages fileRequirements;
in {
  cache = symlinkJoin {
    name = "buffrs-cache";
    paths = allPackages;
  };
}
