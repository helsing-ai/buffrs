{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    crane.url = "github:ipetkov/crane";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    advisory-db = {
      url = "github:rustsec/advisory-db";
      flake = false;
    };
  };

  outputs = { self, flake-utils, rust-overlay, crane, advisory-db, nixpkgs, }:
    let
      perSystemOutputs = flake-utils.lib.eachDefaultSystem (system:
        let
          pkgs = (import nixpkgs) {
            inherit system;
            overlays = [ (import rust-overlay) ];
          };
          inherit (pkgs) lib callPackage;
          rustToolchain = callPackage ./nix/toolchain.nix { };

          darwinFrameworks = with pkgs.darwin.apple_sdk.frameworks; [
            Security
            SystemConfiguration
          ];

          devTools = [ rustToolchain ];

          dependencies = with pkgs;
            [ libiconv ]
            ++ lib.lists.optionals stdenv.isDarwin darwinFrameworks;

          nativeBuildInputs = with pkgs; [ pkg-config ] ++ dependencies;

          buildEnvVars = {
            NIX_LDFLAGS = [ "-L" "${pkgs.libiconv}/lib" ];
            OPENSSL_NO_VENDOR = 1;
          };

          buffrs = callPackage ./nix/buffrs.nix {
            inherit crane advisory-db buildEnvVars nativeBuildInputs
              rustToolchain;

            buildInputs = [ rustToolchain ];
          };

          app = flake-utils.lib.mkApp { drv = buffrs.package; };
        in {
          # NB: if this does not build and you need to modify the file,
          #     please ensure you also make the corresponding changes in the devshell
          packages.default = buffrs.package;
          apps.default = app;

          lib.vendorDependencies =
            pkgs.callPackage ./nix/cache.nix { buffrs = buffrs.package; };

          devShells.default = pkgs.mkShell ({
            nativeBuildInputs = nativeBuildInputs ++ [ pkgs.protobuf ];
            buildInputs = devTools ++ dependencies;
          } // buildEnvVars);

          formatter = with pkgs;
            writeShellApplication {
              name = "nixfmt-nix-files";
              runtimeInputs = [ fd nixfmt-classic ];
              text = "fd \\.nix\\$ --hidden --type f | xargs nixfmt";
            };

          checks = ({
            nix-files-are-formatted = pkgs.stdenvNoCC.mkDerivation {
              name = "fmt-check";
              dontBuild = true;
              src = ./.;
              doCheck = true;
              nativeBuildInputs = with pkgs; [ fd nixfmt-classic ];
              checkPhase = ''
                set -e
                # find all nix files, and verify that they're formatted correctly
                fd \.nix\$ --hidden --type f | xargs nixfmt -c
              '';
              installPhase = ''
                mkdir "$out"
              '';
            };
          } // buffrs.checks);

          overlays.default = (_final: _prev: { buffrs = app; });
        });
    in perSystemOutputs // {
      overlays.default = (final: _prev: {
        buffrs = perSystemOutputs.packages.${final.stdenv.system}.default;
      });
    };
}
