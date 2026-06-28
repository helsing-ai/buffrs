use crate::{VirtualFileSystem, with_test_maven_registry, with_test_registry};

/// Test publishing to Artifactory registry
#[test]
fn publish_to_artifactory() {
    with_test_registry(|artifactory_url| {
        let vfs = VirtualFileSystem::copy(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/projects/lib"),
        );

        // Publish to Artifactory
        crate::cli!()
            .arg("publish")
            .arg("--registry")
            .arg(format!("artifactory+{}", artifactory_url))
            .arg("--repository")
            .arg("artifactory-repo")
            .current_dir(vfs.root())
            .assert()
            .success();
    });
}

/// Test publishing to Maven registry
#[test]
fn publish_to_maven() {
    with_test_maven_registry(|maven_url| {
        let vfs = VirtualFileSystem::copy(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/projects/lib"),
        );

        // Publish to Maven
        crate::cli!()
            .arg("publish")
            .arg("--registry")
            .arg(maven_url)
            .arg("--repository")
            .arg("maven-repo")
            .current_dir(vfs.root())
            .assert()
            .success();
    });
}

/// Test that credentials are isolated by registry type
#[test]
fn credentials_isolation() {
    let vfs = VirtualFileSystem::empty().with_virtual_home();

    // Login to Artifactory registry
    crate::cli!()
        .arg("login")
        .arg("--registry")
        .arg("artifactory+https://my-artifactory.com")
        .current_dir(vfs.root())
        .write_stdin("artifactory-token\n")
        .assert()
        .success();

    // Login to Maven registry
    crate::cli!()
        .arg("login")
        .arg("--registry")
        .arg("maven+https://my-maven.com")
        .current_dir(vfs.root())
        .write_stdin("maven-token\n")
        .assert()
        .success();

    // Verify both credentials are stored
    let credentials_path = vfs
        .root()
        .join(VirtualFileSystem::VIRTUAL_HOME)
        .join(".buffrs/credentials.toml");

    let contents = std::fs::read_to_string(credentials_path).unwrap();

    // Both registry types should be present in credentials
    assert!(contents.contains("artifactory+https://my-artifactory.com"));
    assert!(contents.contains("maven+https://my-maven.com"));
    assert!(contents.contains("artifactory-token"));
    assert!(contents.contains("maven-token"));
}
