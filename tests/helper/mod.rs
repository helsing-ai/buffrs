use std::path::Path;

/// Helper to create, initialize, write a proto file, and publish a library package.
#[allow(clippy::too_many_arguments)]
pub fn publish_test_library(
    cwd: &Path,
    buffrs_home: &Path,
    registry_url: &str,
    repository: &str,
    name: &str,
    version: Option<&str>,
    proto_filename: &str,
    proto_content: &str,
) {
    let lib_dir = cwd.join(name);
    std::fs::create_dir_all(&lib_dir).unwrap();

    crate::cli!()
        .args(["init", "--lib", name])
        .env("BUFFRS_HOME", buffrs_home)
        .current_dir(&lib_dir)
        .assert()
        .success();

    if let Some(version) = version {
        let manifest_path = lib_dir.join("Proto.toml");
        let manifest = std::fs::read_to_string(&manifest_path).unwrap();
        let updated = manifest.replace("version = \"0.1.0\"", &format!("version = \"{version}\""));
        std::fs::write(&manifest_path, updated).unwrap();
    }

    let proto_path = lib_dir.join(format!("proto/{proto_filename}"));
    if let Some(parent) = proto_path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(proto_path, proto_content).unwrap();

    crate::cli!()
        .args([
            "publish",
            "--registry",
            registry_url,
            "--repository",
            repository,
        ])
        .env("BUFFRS_HOME", buffrs_home)
        .current_dir(&lib_dir)
        .assert()
        .success();
}
