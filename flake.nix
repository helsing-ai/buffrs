{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    crane = {
      url = "github:ipetkov/crane";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
      inputs.rust-analyzer-src.follows = "";
    };

    advisory-db = {
      url = "github:rustsec/advisory-db";
      flake = false;
    };
  };

  outputs = { self, flake-utils, fenix, crane, advisory-db, nixpkgs, }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = (import nixpkgs) { inherit system; };
        inherit (pkgs) lib callPackage;
        rustToolchain = callPackage ./.nix/toolchain.nix { inherit fenix; };

        darwinFrameworks = with pkgs.darwin.apple_sdk.frameworks; [
          Security
          SystemConfiguration
        ];

        devTools = [ rustToolchain ];
        libgit2_1_7_2 = callPackage ./.nix/libgit2.nix {
          inherit (pkgs.darwin.apple_sdk.frameworks) Security;
        };

        dependencies = with pkgs;
          [ libgit2_1_7_2 openssl openssl.dev ]
          ++ lib.lists.optionals stdenv.isDarwin darwinFrameworks;

        nativeBuildInputs = with pkgs; [ pkg-config ] ++ dependencies;

        buildEnvVars = {
          pkg_config_path = [ "${libgit2_1_7_2}/lib/pkgconfig" ];
          LIBGIT2_NO_VENDOR = 1;
          OPENSSL_NO_VENDOR = 1;
        };

        buffrs = callPackage ./.nix/buffrs.nix {
          inherit crane advisory-db buildEnvVars nativeBuildInputs
            rustToolchain;
          buildInputs = [ rustToolchain ];
        };
      in {
        # NB: if this does not build and you need to modify the file,
        #     please ensure you also make the corresponding changes in the devshell
        packages.default = buffrs.package;
        apps.default = flake-utils.lib.makeApp { drv = buffrs.package; };

        devShells.default = pkgs.mkShell ({
          inherit nativeBuildInputs;
          buildInputs = devTools ++ dependencies;
        } // buildEnvVars);

        formatter = with pkgs;
          writeShellApplication {
            name = "nixfmt-nix-files";
            runtimeInputs = [ fd nixfmt ];
            text = "fd \\.nix\\$ --hidden --type f | xargs nixfmt";
          };

        checks = ({
          nix-files-are-formatted = pkgs.stdenvNoCC.mkDerivation {
            name = "fmt-check";
            dontBuild = true;
            src = ./.;
            doCheck = true;
            nativeBuildInputs = with pkgs; [ fd nixfmt ];
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
      });
}
