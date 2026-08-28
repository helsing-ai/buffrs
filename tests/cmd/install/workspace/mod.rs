mod creates_lockfile;
mod lockfile;
mod lockfile_diamond_dependencies;
mod lockfile_multiple_versions;
mod lockfile_transitive;
mod range_caret_resolution;
mod range_cascade_resolution;
mod range_compatible_diamond;
mod range_downgrade_diamond;
mod range_incompatible_diamond;
mod range_lockfile_reresolution;
mod range_multi_level_tree;
mod range_tilde_resolution;

/// Publishes a dependency-free lib at `version` and removes its scratch directory.
pub(super) fn publish_leaf_lib(
    cwd: &std::path::Path,
    buffrs_home: &std::path::Path,
    url: &str,
    name: &str,
    version: &str,
) {
    publish_lib(cwd, buffrs_home, url, name, version, None);
}

/// Publishes a lib at `version` that depends on `dep_name@dep_req`.
pub(super) fn publish_lib_with_dep(
    cwd: &std::path::Path,
    buffrs_home: &std::path::Path,
    url: &str,
    name: &str,
    version: &str,
    dep_name: &str,
    dep_req: &str,
) {
    publish_lib(
        cwd,
        buffrs_home,
        url,
        name,
        version,
        Some((dep_name, dep_req)),
    );
}

fn publish_lib(
    cwd: &std::path::Path,
    buffrs_home: &std::path::Path,
    url: &str,
    name: &str,
    version: &str,
    dependency: Option<(&str, &str)>,
) {
    let dir = cwd.join(format!("{name}-v{version}"));
    std::fs::create_dir(&dir).unwrap();

    crate::cli!()
        .args(["init", "--lib", name])
        .env("BUFFRS_HOME", buffrs_home)
        .current_dir(&dir)
        .assert()
        .success();

    let manifest_path = dir.join("Proto.toml");
    let manifest = std::fs::read_to_string(&manifest_path)
        .unwrap()
        .replace("version = \"0.1.0\"", &format!("version = \"{version}\""));
    std::fs::write(&manifest_path, manifest).unwrap();

    let proto_package = name.replace('-', "_");
    std::fs::write(
        dir.join(format!("proto/{proto_package}.proto")),
        format!(
            "syntax = \"proto3\";\n\npackage {proto_package};\n\nmessage M {{ string value = 1; }}\n"
        ),
    )
    .unwrap();

    if let Some((dep_name, dep_req)) = dependency {
        crate::cli!()
            .args([
                "add",
                "--registry",
                url,
                &format!("test-repo/{dep_name}@{dep_req}"),
            ])
            .env("BUFFRS_HOME", buffrs_home)
            .current_dir(&dir)
            .assert()
            .success();
    }

    crate::cli!()
        .args(["publish", "--registry", url, "--repository", "test-repo"])
        .env("BUFFRS_HOME", buffrs_home)
        .current_dir(&dir)
        .assert()
        .success();

    // Scratch directories must not linger in the workspace root.
    std::fs::remove_dir_all(&dir).unwrap();
}
