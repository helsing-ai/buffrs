{ pkgs, crane, rustToolchain, system, advisory-db, buildInputs
, nativeBuildInputs, buildEnvVars }:
let
  craneLib = crane.lib.${system};
  src = craneLib.cleanCargoSource (craneLib.path ../.);

  # Common arguments can be set here to avoid repeating them later
  commonArgs = {
    inherit src buildInputs nativeBuildInputs;
    strictDeps = true;
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

    # Run clippy (and deny all warnings) on the crate source,
    # again, resuing the dependency artifacts from above.
    #
    # Note that this is done as a separate derivation so that
    # we can block the CI if there are issues here, but not
    # prevent downstream consumers from building our crate by itself.
    buffrs-clippy = craneLib.cargoClippy (commonArgs // {
      inherit cargoArtifacts;
      cargoClippyExtraArgs =
        "--all-targets --workspace -- -D warnings -D clippy::all";
    });

    # Check formatting
    buffrs-fmt = craneLib.cargoFmt {
      inherit src;
      cargoDenyExtraArgs = "--workspace check";
    };

    # Audit dependencies
    buffrs-audit = craneLib.cargoAudit {
      inherit src advisory-db;
      # ignoring as no workaround is currently available
      cargoAuditExtraArgs = "--ignore RUSTSEC-2023-0071";
    };

    # Audit licenses
    buffrs-deny = craneLib.cargoDeny { inherit src; };

    buffrs-nextest = craneLib.cargoNextest (commonArgs // {
      inherit cargoArtifacts;
      partitions = 1;
      partitionType = "count";
    });
  };
}
