{
  inputs = {
    flake-utils.url = "github:numtide/flake-utils";
    naersk.url = "github:nix-community/naersk";
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
  };

  outputs = {
    self,
    flake-utils,
    naersk,
    nixpkgs,
  }:
    flake-utils.lib.eachDefaultSystem (
      system: let
        pkgs = (import nixpkgs) {
          inherit system;
        };
        inherit (pkgs) lib;

        naersk' = pkgs.callPackage naersk {};
        nativeBuildInputs = with pkgs; [pkg-config];

        darwin_frameworks = with pkgs.darwin.apple_sdk.frameworks; [
          Security
          SystemConfiguration
        ];

        dev_tools = with pkgs; [
          cargo
          rustc
        ];

        dependencies = with pkgs;
          [
            libgit2
            openssl
          ]
          ++ lib.lists.optionals stdenv.isDarwin darwin_frameworks;

        env_vars = {
          LIBGIT2_NO_VENDOR = 1;
          OPENSSL_NO_VENDOR = 1;
        };
      in rec {
        packages.default = naersk'.buildPackage ({
            inherit nativeBuildInputs;
            src = ./.;
            buildInputs = dev_tools ++ dependencies;
          }
          // env_vars);
        devShells.default = pkgs.mkShell ({
            buildInputs = nativeBuildInputs ++ dev_tools ++ dependencies;
          }
          // env_vars);
        checks.builds = packages.default;
      }
    );
}
