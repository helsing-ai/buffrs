use crate::{with_test_registry, VirtualFileSystem};
use std::process::Command;

#[test]
fn fixture() {
    with_test_registry(|url| {
        let vfs = VirtualFileSystem::empty();
        let buffrs_home = vfs.root().join("$HOME");
        let cwd = vfs.root();

        // Units Library
        {
            // mkdir units
            std::fs::create_dir(cwd.join("units")).unwrap();

            // cd units
            let cwd = cwd.join("units");

            // buffrs init --lib units
            crate::cli!()
                .args(["init", "--lib", "units"])
                .env("BUFFRS_HOME", &buffrs_home)
                .current_dir(&cwd)
                .assert()
                .success();

            // ed proto/temperature.proto
            std::fs::copy(
                crate::parent_directory!().join("in/temperature.proto"),
                cwd.join("proto/temperature.proto"),
            )
            .unwrap();

            // buffrs publish --repository physics
            crate::cli!()
                .args(["publish", "--registry", url, "--repository", "physics"])
                .env("BUFFRS_HOME", &buffrs_home)
                .current_dir(&cwd)
                .assert()
                .success();
        }

        // Sensor API
        {
            // mkdir sensor-api
            std::fs::create_dir(cwd.join("sensor-api")).unwrap();

            // cd sensor-api
            let cwd = cwd.join("sensor-api");

            // buffrs init --lib sensor-api
            crate::cli!()
                .args(["init", "--api", "sensor-api"])
                .env("BUFFRS_HOME", &buffrs_home)
                .current_dir(&cwd)
                .assert()
                .success();

            // buffrs add physics/units@=0.1.0
            crate::cli!()
                .args(["add", "--registry", url, "physics/units@=0.1.0"])
                .env("BUFFRS_HOME", &buffrs_home)
                .current_dir(&cwd)
                .assert()
                .success();

            // buffrs install
            crate::cli!()
                .arg("install")
                .env("BUFFRS_HOME", &buffrs_home)
                .current_dir(&cwd)
                .assert()
                .success();

            // ed proto/sensor.proto
            std::fs::copy(
                crate::parent_directory!().join("in/sensor.proto"),
                cwd.join("proto/sensor.proto"),
            )
            .unwrap();

            // buffrs publish --repository iot
            crate::cli!()
                .args(["publish", "--registry", url, "--repository", "iot"])
                .env("BUFFRS_HOME", &buffrs_home)
                .current_dir(&cwd)
                .assert()
                .success();
        }

        // Sensor Server
        {
            // cargo init sensor-server
            assert!(Command::new("cargo")
                .args(["init", "sensor-server"])
                .current_dir(&cwd)
                .status()
                .unwrap()
                .success());

            // cd sensor-server
            let cwd = cwd.join("sensor-server");

            // buffrs init
            crate::cli!()
                .arg("init")
                .env("BUFFRS_HOME", &buffrs_home)
                .current_dir(&cwd)
                .assert()
                .success();

            // buffrs add iot/sensor-api@=0.1.0
            crate::cli!()
                .args(["add", "--registry", url, "iot/sensor-api@=0.1.0"])
                .env("BUFFRS_HOME", &buffrs_home)
                .current_dir(&cwd)
                .assert()
                .success();

            // buffrs install
            crate::cli!()
                .arg("install")
                .env("BUFFRS_HOME", &buffrs_home)
                .current_dir(&cwd)
                .assert()
                .success();

            // ed Cargo.toml
            std::fs::copy(
                crate::parent_directory!().join("in/Cargo.toml"),
                cwd.join("Cargo.toml"),
            )
            .unwrap();

            let path = std::env::current_dir()
                .unwrap()
                .join(crate::parent_directory!());
            let git_repo = gix::discover(path).expect("unable to find git root");
            let git_root = git_repo
                .path()
                .parent()
                .expect("failed to move from path to .git to its parent")
                .to_str()
                .expect("failed to convert path git root to string");
            dbg!(git_root);

            // cargo add buffrs --no-default-features
            assert!(Command::new("cargo")
                .args(["add", "buffrs", "--no-default-features", "--path", git_root])
                .current_dir(&cwd)
                .status()
                .unwrap()
                .success());

            // cargo add --build buffrs --features=build
            assert!(Command::new("cargo")
                .args([
                    "add",
                    "buffrs",
                    "--build",
                    "--no-default-features",
                    "--path",
                    git_root
                ])
                .current_dir(&cwd)
                .status()
                .unwrap()
                .success());

            // ed build.rs
            std::fs::copy(
                crate::parent_directory!().join("in/build.rs"),
                cwd.join("build.rs"),
            )
            .unwrap();

            // ed src/main.rs
            std::fs::copy(
                crate::parent_directory!().join("in/main.rs"),
                cwd.join("src/main.rs"),
            )
            .unwrap();

            // cargo build
            assert!(Command::new("cargo")
                .arg("build")
                .current_dir(&cwd)
                .status()
                .unwrap()
                .success());
        }
    });
}
