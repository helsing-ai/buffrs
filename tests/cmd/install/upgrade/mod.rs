use crate::{VirtualFileSystem, with_test_registry};

#[test]
fn fixture() {
    with_test_registry(|url| {
        let vfs = VirtualFileSystem::copy(crate::parent_directory!().join("in"));
        let buffrs_home = vfs.root().join("$HOME");
        let cwd = vfs.root();

        // Dummy Library
        {
            // mkdir dummy
            std::fs::create_dir(cwd.join("dummy")).unwrap();

            // cd dummy
            let cwd = cwd.join("dummy");

            // buffrs init --lib dummy
            crate::cli!()
                .args(["init", "--lib", "dummy"])
                .env("BUFFRS_HOME", &buffrs_home)
                .current_dir(&cwd)
                .assert()
                .success();

            // ed proto/dummy.proto
            std::fs::copy(
                crate::parent_directory!().join("in/dummy.proto"),
                cwd.join("proto/dummy.proto"),
            )
            .unwrap();

            // publish version 0.1.0
            // buffrs publish --repository dummy
            crate::cli!()
                .args(["publish", "--registry", url, "--repository", "dummy"])
                .env("BUFFRS_HOME", &buffrs_home)
                .current_dir(&cwd)
                .assert()
                .success();

            let manifest = std::fs::read_to_string(cwd.join("Proto.toml")).unwrap();
            let updated_manifest = manifest.replacen("0.1.0", "0.2.0", 1);
            std::fs::write(cwd.join("Proto.toml"), updated_manifest).unwrap();

            // publish version 0.2.0
            // buffrs publish --repository dummy
            crate::cli!()
                .args(["publish", "--registry", url, "--repository", "dummy"])
                .env("BUFFRS_HOME", &buffrs_home)
                .current_dir(&cwd)
                .assert()
                .success();
        }

        crate::cli!()
            .args(["add", "--registry", url, "dummy/dummy@=0.1.0"])
            .env("BUFFRS_HOME", &buffrs_home)
            .current_dir(&cwd)
            .assert()
            .success();

        crate::cli!()
            .arg("install")
            .current_dir(vfs.root())
            .assert()
            .success();

        // Upgrade dependency from 0.1.0 to 0.2.0
        let manifest = std::fs::read_to_string(cwd.join("Proto.toml")).unwrap();
        let updated_manifest = manifest.replacen("0.1.0", "0.2.0", 1);
        std::fs::write(cwd.join("Proto.toml"), updated_manifest).unwrap();

        crate::cli!()
            .arg("install")
            .current_dir(vfs.root())
            .assert()
            .success()
            .stdout(include_str!("stdout.log"))
            .stderr(include_str!("stderr.log"));
    })
}
