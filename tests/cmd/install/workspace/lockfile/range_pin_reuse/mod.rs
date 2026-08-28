use crate::{VirtualFileSystem, with_test_registry};

/// A range requirement whose pin is still satisfiable must be reused from the
/// lockfile instead of being re-resolved against the registry.
///
/// The sibling `stale` fixture only exercises exact pins (`=0.1.0`), which take
/// the equality path. Here pkg1 depends on `lib-a@^1.0.0`: the first install
/// pins 1.2.0, then 1.3.0 is published. A second install must still yield
/// 1.2.0 — proof that the pin was matched by
/// `Lockfile::find_satisfying_all` and the registry was never asked for a
/// version, since asking would have returned the newer 1.3.0.
#[test]
fn fixture() {
    with_test_registry(|url| {
        let vfs = VirtualFileSystem::copy(crate::parent_directory!().join("in"));
        let buffrs_home = vfs.root().join("$HOME");
        let cwd = vfs.root();

        for version in ["1.0.0", "1.2.0"] {
            super::super::publish_leaf_lib(&cwd, &buffrs_home, url, "lib-a", version);
        }

        crate::cli!()
            .args(["add", "--registry", url, "test-repo/lib-a@^1.0.0"])
            .env("BUFFRS_HOME", &buffrs_home)
            .current_dir(cwd.join("pkg1"))
            .assert()
            .success();

        crate::cli!()
            .arg("install")
            .env("BUFFRS_HOME", &buffrs_home)
            .current_dir(&cwd)
            .assert()
            .success();

        let lib_a = |lock: &str| {
            lock.split("[[packages]]")
                .find(|s| s.contains("name = \"lib-a\""))
                .expect("lib-a in lockfile")
                .to_string()
        };

        let lockfile_path = cwd.join("Proto.lock");
        let after_first = std::fs::read_to_string(&lockfile_path).unwrap();
        assert!(
            lib_a(&after_first).contains("version = \"1.2.0\""),
            "first install pins the highest version satisfying ^1.0.0: {after_first}"
        );

        // A newer compatible version appears after the pin was written. Only a
        // fresh registry resolution would pick it up.
        super::super::publish_leaf_lib(&cwd, &buffrs_home, url, "lib-a", "1.3.0");

        crate::cli!()
            .arg("install")
            .env("BUFFRS_HOME", &buffrs_home)
            .current_dir(&cwd)
            .assert()
            .success();

        let after_second = std::fs::read_to_string(&lockfile_path).unwrap();
        assert!(
            lib_a(&after_second).contains("version = \"1.2.0\""),
            "second install must reuse the 1.2.0 pin, not upgrade to 1.3.0: {after_second}"
        );
        assert!(
            !after_second.contains("version = \"1.3.0\""),
            "1.3.0 must not enter the lockfile while a satisfying pin exists: {after_second}"
        );
        assert_eq!(
            lib_a(&after_first),
            lib_a(&after_second),
            "the lib-a entry (version and digest) must survive the second install unchanged"
        );
    })
}
