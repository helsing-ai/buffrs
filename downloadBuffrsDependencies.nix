{fetchurl, runCommand, lib, buffrs, symlinkJoin}:
{lockfile}:
let
src = pkgs.runCommand "vendor-lockfile" {} ''
  mkdir -p $out
  cp ${lockfile} $out/Proto.lock
'';
fileRequirementsJson = pkgs.runCommand "buffrs-urls" {
    buildInputs = [ buffrs ];
} ''
  cd ${src}
  buffrs urls > $out
'';
fileRequirements = builtins.fromJSON (builtins.readFile fileRequirementsJson);
fetchBuffr = (fileRequirement: let
    prefix = "sha256";
    sha256 = assert lib.strings.hasPrefix prefix fileRequirement.digest; lib.strings.removePrefix prefix fileRequirement.digest;

    in fetchurl {
        inherit sha256;
        url = fileRequirement.url;
        downloadToTemp = true;
        postFetch = ''
        mkdir -p $out
        mv $downloadedFile $out/sha256-${sha256}.tgz
        '';
    });
buffrs = map fetchBuffr urls;
buffrsCache = symlinkJoin {
    name = "buffrs-cache";
    paths = buffs;
};
in {
    BUFFRS_CACHE_DIR = "${buffrsCache}";
}

