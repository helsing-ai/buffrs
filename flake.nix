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
        nativeBuildInputs = with pkgs; [ pkg-config ];
      in rec {
        packages.default = naersk'.buildPackage {
          inherit nativeBuildInputs;
          src = ./.;
          buildInputs = with pkgs; [ openssl ];
        };
        devShells.default = pkgs.mkShell {
          nativeBuildInputs = with pkgs; nativeBuildInputs ++ [
            cargo
            rustc
          ];
        };
        checks.builds = packages.default;
      }
    );
}
