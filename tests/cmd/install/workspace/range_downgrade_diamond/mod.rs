use crate::{VirtualFileSystem, with_test_registry};

/// Transitive diamond inside a single package's graph, where the highest
/// available version satisfies the first-encountered requirement but not the
/// second.
///
///   pkg1 --(^1.0.0)--> lib-x --(^1.0.0)--> lib-a
///   pkg1 --(^1.0.0)--> lib-y --(<=1.2.0)-> lib-a
///
/// lib-a is published at 1.0.0, 1.2.0 and 1.5.0. First-encounter-wins resolves
/// lib-a to 1.5.0 via lib-x and then fails against lib-y's <=1.2.0.
/// Merge-and-resolve must intersect both requirements and pick 1.2.0.
///
/// The diamond is expressed transitively on purpose: workspace members are
/// resolved independently by design, so two members are two resolutions rather
/// than one merge.
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

        for dep in ["lib-x", "lib-y"] {
            crate::cli!()
                .args(["add", "--registry", url, &format!("test-repo/{dep}@^1.0.0")])
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

        assert_eq!(
            lockfile.matches("name = \"lib-a\"").count(),
            1,
            "lib-a must be resolved to a single version: {lockfile}"
        );

        let section = lockfile
            .split("[[packages]]")
            .find(|s| s.contains("name = \"lib-a\""))
            .expect("lib-a in lockfile");

        assert!(
            section.contains("version = \"1.2.0\""),
            "expected lib-a 1.2.0, got: {section}"
        );
    })
}
