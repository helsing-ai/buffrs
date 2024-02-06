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
        inherit (pkgs) lib;

        naersk' = pkgs.callPackage naersk {};
        nativeBuildInputs = with pkgs; [ pkg-config ];

        darwin_frameworks = with pkgs.darwin.apple_sdk.frameworks; [
          Security
          SystemConfiguration
        ];
      in rec {
        packages.default = naersk'.buildPackage {
          inherit nativeBuildInputs;
          src = ./.;
          buildInputs = with pkgs; [ openssl perl ] ++ lib.lists.optionals stdenv.isDarwin darwin_frameworks;

          OPENSSL_NO_VENDOR = 1;
        };
        devShells.default = pkgs.mkShell {
          LIBGIT2_NO_VENDOR = 1;
          buildInputs = with pkgs; nativeBuildInputs ++ [
            cargo
            libgit2
            libiconv
            rustc
          ] ++ lib.lists.optionals stdenv.isDarwin darwin_frameworks;
        };
        checks.builds = packages.default;
      }
    );
}
