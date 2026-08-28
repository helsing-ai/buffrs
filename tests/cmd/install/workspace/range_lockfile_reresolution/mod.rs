use crate::{VirtualFileSystem, with_test_registry};

/// The first install pins lib-a@1.5.0 via lib-x's ^1.0.0. pkg1 then gains
/// lib-y, whose <=1.2.0 the pin no longer satisfies. The second install must
/// re-resolve lib-a to 1.2.0 and rewrite the lockfile — a pin that fails the
/// merged requirements is not an error, it is simply not a usable answer.
///
/// This matters for workspaces in particular: the workspace lockfile is a pool
/// shared by independently-resolved members, so an entry that satisfies nothing
/// here may legitimately belong to another member.
#[test]
fn fixture() {
    with_test_registry(|url| {
        let vfs = VirtualFileSystem::copy(crate::parent_directory!().join("in"));
        let buffrs_home = vfs.root().join("$HOME");
        let cwd = vfs.root();

        for version in ["1.0.0", "1.2.0", "1.5.0"] {
            super::publish_leaf_lib(&cwd, &buffrs_home, url, "lib-a", version);
        }
        super::publish_lib_with_dep(&cwd, &buffrs_home, url, "lib-x", "1.0.0", "lib-a", "^1.0.0");
        super::publish_lib_with_dep(
            &cwd,
            &buffrs_home,
            url,
            "lib-y",
            "1.0.0",
            "lib-a",
            "<=1.2.0",
        );

        crate::cli!()
            .args(["add", "--registry", url, "test-repo/lib-x@^1.0.0"])
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

        let lockfile = std::fs::read_to_string(cwd.join("Proto.lock")).unwrap();
        assert!(
            lib_a(&lockfile).contains("version = \"1.5.0\""),
            "first install pins the highest version satisfying ^1.0.0: {lockfile}"
        );

        // Introduce a second path to lib-a that the existing pin cannot satisfy.
        crate::cli!()
            .args(["add", "--registry", url, "test-repo/lib-y@^1.0.0"])
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

        let lockfile = std::fs::read_to_string(cwd.join("Proto.lock")).unwrap();
        assert_eq!(
            lockfile.matches("name = \"lib-a\"").count(),
            1,
            "lib-a must be re-pinned, not duplicated: {lockfile}"
        );
        assert!(
            lib_a(&lockfile).contains("version = \"1.2.0\""),
            "stale pin must be re-resolved to 1.2.0: {}",
            lib_a(&lockfile)
        );
        assert!(
            lockfile.contains("name = \"lib-x\"") && lockfile.contains("name = \"lib-y\""),
            "both intermediates must remain locked: {lockfile}"
        );
    })
}
