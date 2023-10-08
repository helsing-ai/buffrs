{
  inputs = {
    flake-utils.url = "github:numtide/flake-utils";
    naersk.url = "github:nix-community/naersk";
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
  };

  outputs = { self, flake-utils, naersk, nixpkgs }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = (import nixpkgs) {
          inherit system;
        };
        naersk' = pkgs.callPackage naersk {};
      in rec {
        defaultPackage = naersk'.buildPackage {
          src = ./.;
          buildInputs = with pkgs; [ openssl ];
          nativeBuildInputs = with pkgs; [
            pkg-config
          ];
        };
        devShell = pkgs.mkShell {
          nativeBuildInputs = with pkgs; defaultPackage.nativeBuildInputs ++ [
            cargo
            rustc
          ];
        };
        checks.builds = defaultPackage;
      }
    );
}
