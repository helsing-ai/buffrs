use crate::{VirtualFileSystem, with_test_registry};

/// Cascade case: a transitive dep adds a tighter requirement *after* the
/// resolver has already chosen a higher version of the shared package and
/// walked its transitives. The resolver must:
///   1. Re-resolve the shared package to the lower version.
///   2. Retract requirement edges contributed by the now-discarded version.
///   3. Drop any package that becomes unreachable as a result.
///   4. Walk the new version's transitives.
///
/// Setup, all inside pkg1's graph:
///   lib-a@1.5.0 -> lib-b ^2.0.0     lib-a@1.2.0 -> lib-b ^1.0.0
///   mid@1.0.0   -> lib-a ~1.2.0
///   pkg1        -> lib-a ^1.0.0, mid ^1.0.0
///
/// Root deps are traversed sorted, so lib-a resolves to 1.5.0 and pulls in
/// lib-b@2.0.0 before mid contributes ~1.2.0. lib-a must then fall to 1.2.0,
/// lib-b@2.0.0 must be retracted, and lib-b@1.0.0 walked in its place.
#[test]
fn fixture() {
    with_test_registry(|url| {
        let vfs = VirtualFileSystem::copy(crate::parent_directory!().join("in"));
        let buffrs_home = vfs.root().join("$HOME");
        let cwd = vfs.root();

        for version in ["1.0.0", "2.0.0"] {
            super::publish_leaf_lib(&cwd, &buffrs_home, url, "lib-b", version);
        }
        super::publish_lib_with_dep(&cwd, &buffrs_home, url, "lib-a", "1.5.0", "lib-b", "^2.0.0");
        super::publish_lib_with_dep(&cwd, &buffrs_home, url, "lib-a", "1.2.0", "lib-b", "^1.0.0");
        super::publish_lib_with_dep(&cwd, &buffrs_home, url, "mid", "1.0.0", "lib-a", "~1.2.0");

        for spec in ["test-repo/lib-a@^1.0.0", "test-repo/mid@^1.0.0"] {
            crate::cli!()
                .args(["add", "--registry", url, spec])
                .env("BUFFRS_HOME", &buffrs_home)
                .current_dir(cwd.join("pkg1"))
                .assert()
                .success();
        }

        crate::cli!()
            .arg("install")
            .env("BUFFRS_HOME", &buffrs_home)
            .current_dir(&cwd)
            .assert()
            .success();

        let lockfile = std::fs::read_to_string(cwd.join("Proto.lock")).unwrap();
        let section = |name: &str| {
            lockfile
                .split("[[packages]]")
                .find(|s| s.contains(&format!("name = \"{name}\"")))
                .unwrap_or_else(|| panic!("{name} missing from lockfile: {lockfile}"))
                .to_string()
        };

        assert!(
            section("lib-a").contains("version = \"1.2.0\""),
            "lib-a must downgrade: {}",
            section("lib-a")
        );
        assert!(
            section("lib-b").contains("version = \"1.0.0\""),
            "lib-b must follow the new lib-a: {}",
            section("lib-b")
        );
        assert!(
            !lockfile.contains("version = \"2.0.0\""),
            "retracted lib-b@2.0.0 leaked into the lockfile: {lockfile}"
        );
    })
}
