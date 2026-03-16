mod package;
mod workspace;

use std::path::{Path, PathBuf};
use std::process::Command;

type NixScript = &'static str;

/// A generated nix flake that uses the buffrs `vendorDependencies` mechanism
/// to download and cache protobuf dependencies, then runs a user-provided
/// install script inside a sandboxed derivation (no network access).
struct Flake {
    repo: String,
    name: String,
    script: String,
    path: Option<PathBuf>,
}

struct FlakeBuilder<R, N, S> {
    repo: R,
    name: N,
    script: S,
}

impl Flake {
    fn builder() -> FlakeBuilder<(), (), ()> {
        FlakeBuilder {
            repo: (),
            name: (),
            script: (),
        }
    }

    /// Render the flake.nix content.
    fn render(&self) -> String {
        let repo = &self.repo;
        let name = &self.name;
        let script = &self.script;

        format!(
            r#"{{
  inputs = {{
    buffrs.url = "path:{repo}";
    nixpkgs.follows = "buffrs/nixpkgs";
    flake-utils.follows = "buffrs/flake-utils";
  }};

  outputs = {{ buffrs, nixpkgs, flake-utils, ... }}:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs {{ inherit system; }};
        vendored = buffrs.lib.${{system}}.vendorDependencies ./Proto.lock;
        buffrs-bin = buffrs.packages.${{system}}.default;

        installed = pkgs.runCommand "{name}" ({{
          buildInputs = [ buffrs-bin ];
        }} // vendored) ''
{script}
        '';
      in {{
        packages.default = installed;
        checks.default = installed;
      }}
    );
}}"#
        )
    }

    /// Write flake.nix to a directory and initialize it as a git repo
    /// (required by nix flakes).
    fn write(&mut self, path: &Path) {
        std::fs::write(path.join("flake.nix"), self.render()).unwrap();

        assert!(
            Command::new("git")
                .args(["init"])
                .current_dir(path)
                .status()
                .expect("git not found")
                .success(),
            "git init failed"
        );

        assert!(
            Command::new("git")
                .args(["add", "."])
                .current_dir(path)
                .status()
                .expect("git not found")
                .success(),
            "git add failed"
        );

        self.path = Some(path.to_path_buf());
    }

    fn path(&self) -> &Path {
        self.path
            .as_deref()
            .expect("flake not yet written — call write() first")
    }

    /// Run `nix build` and return the `result` symlink path.
    fn build(&self) -> PathBuf {
        let output = Command::new("nix")
            .args(["build", "--print-build-logs"])
            .current_dir(self.path())
            .output()
            .expect("nix not found — this test requires nix to be installed");

        assert!(
            output.status.success(),
            "nix build failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );

        self.path().join("result")
    }

    /// Run `nix flake check`.
    fn check(&self) {
        let output = Command::new("nix")
            .args(["flake", "check", "--print-build-logs"])
            .current_dir(self.path())
            .output()
            .expect("nix flake check failed");

        assert!(
            output.status.success(),
            "nix flake check failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

impl<N, S> FlakeBuilder<(), N, S> {
    fn repo(self, repo: impl Into<String>) -> FlakeBuilder<String, N, S> {
        FlakeBuilder {
            repo: repo.into(),
            name: self.name,
            script: self.script,
        }
    }
}

impl<R, S> FlakeBuilder<R, (), S> {
    fn name(self, name: impl Into<String>) -> FlakeBuilder<R, String, S> {
        FlakeBuilder {
            repo: self.repo,
            name: name.into(),
            script: self.script,
        }
    }
}

impl<R, N> FlakeBuilder<R, N, ()> {
    fn script(self, script: impl Into<String>) -> FlakeBuilder<R, N, String> {
        FlakeBuilder {
            repo: self.repo,
            name: self.name,
            script: script.into(),
        }
    }
}

impl FlakeBuilder<String, String, String> {
    fn build(self) -> Flake {
        Flake {
            repo: self.repo,
            name: self.name,
            script: self.script,
            path: None,
        }
    }
}

mod publish {
    use super::*;

    /// Helper to create, init, and publish a library package to the test registry.
    pub fn lib(
        cwd: &Path,
        buffrs_home: &Path,
        url: &str,
        name: &str,
        proto_filename: &str,
        proto_content: &str,
    ) {
        std::fs::create_dir(cwd.join(name)).unwrap();
        let lib_dir = cwd.join(name);

        crate::cli!()
            .args(["init", "--lib", name])
            .env("BUFFRS_HOME", buffrs_home)
            .current_dir(&lib_dir)
            .assert()
            .success();

        std::fs::write(
            lib_dir.join(format!("proto/{proto_filename}")),
            proto_content,
        )
        .unwrap();

        crate::cli!()
            .args(["publish", "--registry", url, "--repository", "test-repo"])
            .env("BUFFRS_HOME", buffrs_home)
            .current_dir(&lib_dir)
            .assert()
            .success();
    }
}
