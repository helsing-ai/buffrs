{fetchurl, runCommand, lib, buffrs, symlinkJoin}:
 lockfile:
 let
 src = runCommand "vendor-lockfile" {} ''
   mkdir -p $out
   cp ${lockfile} $out/Proto.lock
 '';
 fileRequirementsJson = runCommand "buffrs-urls" {
     buildInputs = [ buffrs ];
 } ''
   cd ${src}
   buffrs dependencies > $out
 '';
 fileRequirements = builtins.fromJSON (builtins.readFile fileRequirementsJson);
 fetchBuffr = (fileRequirement: let
     prefix = "sha256:";
     sha256 = assert lib.strings.hasPrefix prefix fileRequirement.digest; lib.strings.removePrefix prefix fileRequirement.digest;

     buffrTgz = fetchurl {
         inherit sha256;
         url = fileRequirement.url;
     };
     in runCommand "extract" {} ''
       mkdir -p $out
       cp ${buffrTgz} $out/sha256-${sha256}.tgz
     '');
 allBuffrs = map fetchBuffr fileRequirements;
 buffrsCache = symlinkJoin {
     name = "buffrs-cache";
     paths = allBuffrs;
 };
 in {
     BUFFRS_CACHE_DIR = "${buffrsCache}";
 }
