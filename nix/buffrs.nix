{ pkgs, crane, rustToolchain, system, advisory-db, buildInputs
, nativeBuildInputs, buildEnvVars }:
let
  craneLib = crane.lib.${system}.overrideToolchain rustToolchain;
  src = ../.;

  # Common arguments can be set here to avoid repeating them later
  commonArgs = {
    inherit src buildInputs nativeBuildInputs;
    strictDeps = false;
  } // buildEnvVars;

  # Build *just* the cargo dependencies, so we can reuse
  # all of that work (e.g. via cachix) when running in CI
  cargoArtifacts = craneLib.buildDepsOnly commonArgs;

  # Build the actual crate itself, reusing the dependency
  # artifacts from above.
  buffrs = craneLib.buildPackage (commonArgs // {
    inherit cargoArtifacts;
    # we don't want to run tests twice
    doCheck = false;
  });
in {
  package = buffrs;

  checks = {
    # Build the crate as part of `nix flake check` for convenience
    buffrs = buffrs;

    # Audit dependencies
    buffrs-audit = craneLib.cargoAudit {
      inherit src advisory-db;
      # ignoring as no workaround is currently available
      cargoAuditExtraArgs = "--ignore RUSTSEC-2023-0071";
    };

    # Audit licenses
    buffrs-deny = craneLib.cargoDeny { inherit src; };

    # Rust unit and integration tests
    buffers-nextest = craneLib.cargoNextest (commonArgs // {
      inherit cargoArtifacts;
      partitions = 1;
      partitionType = "count";
      # Ignore tutorial test because it requires git and cargo to work
      cargoNextestExtraArgs =
        "--filter-expr 'all() - test(=cmd::tuto::fixture)'";
      SSL_CERT_FILE = "${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt";
    });
  };
}
