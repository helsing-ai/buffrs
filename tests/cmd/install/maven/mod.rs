use crate::{VirtualFileSystem, with_test_maven_registry};

/// Test installing a package from Maven registry
#[test]
fn install_from_maven() {
    with_test_maven_registry(|maven_url| {
        // First, publish a library package to Maven
        let lib_vfs = VirtualFileSystem::copy(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/projects/lib"),
        );

        crate::cli!()
            .arg("publish")
            .arg("--registry")
            .arg(maven_url)
            .arg("--repository")
            .arg("maven-repo")
            .current_dir(lib_vfs.root())
            .assert()
            .success();

        // Now create a consumer package that depends on the library
        let consumer_vfs = VirtualFileSystem::empty();

        // Initialize a new package
        crate::cli!()
            .arg("init")
            .arg("--api")
            .current_dir(consumer_vfs.root())
            .assert()
            .success();

        // Add the dependency
        crate::cli!()
            .arg("add")
            .arg("--registry")
            .arg(maven_url)
            .arg("maven-repo/lib@=0.0.1")
            .current_dir(consumer_vfs.root())
            .assert()
            .success();

        // Install dependencies
        crate::cli!()
            .arg("install")
            .current_dir(consumer_vfs.root())
            .assert()
            .success();

        // Verify the package was installed
        let vendor_path = consumer_vfs.root().join("proto/vendor/lib");
        assert!(vendor_path.exists(), "Vendor directory should exist");
    });
}
