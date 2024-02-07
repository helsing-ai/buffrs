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

        darwinFrameworks = with pkgs.darwin.apple_sdk.frameworks; [
          Security
          SystemConfiguration
        ];

        devTools = with pkgs; [
          cargo
          rustc
        ];

        dependencies = with pkgs;
          [
            libgit2
            openssl
          ]
          ++ lib.lists.optionals stdenv.isDarwin darwinFrameworks;

        envVars = {
          LIBGIT2_NO_VENDOR = 1;
          OPENSSL_NO_VENDOR = 1;
        };
      in rec {
        # NB: if this does not build and you need to modify the file,
        #     please ensure you also make the corresponding changes in the devshell
        packages.default = naersk'.buildPackage ({
            inherit nativeBuildInputs;
            src = ./.;
            buildInputs = devTools ++ dependencies;
          }
          // envVars);

        devShells.default = pkgs.mkShell ({
            buildInputs = nativeBuildInputs ++ devTools ++ dependencies;
          }
          // envVars);

        formatter = pkgs.alejandra;

        checks = {
          builds = packages.default;
          nix-files-are-formatted = pkgs.stdenvNoCC.mkDerivation {
            name = "fmt-check";
            dontBuild = true;
            src = ./.;
            doCheck = true;
            nativeBuildInputs = with pkgs; [alejandra];
            checkPhase = ''
              alejandra -c .
            '';
            installPhase = ''
              mkdir "$out"
            '';
          };
        };
      }
    );
}
