use crate::VirtualFileSystem;

/// This is a finicky test that depends on the contents of lib-0.0.1.tgz. It will fail if the crate version changes.
///
/// If you need to change the version, do the following:
/// PACKAGE_VERSION=0.12 && mkdir tmp && tar -xzf lib-0.0.1.tgz -C tmp && sed -i '' "s/^edition = .*/edition = \"$PACKAGE_VERSION\"/" tmp/Proto.toml && COPYFILE_DISABLE=1 tar -czf lib-0.0.1.tgz -C tmp . && rm -rf tmp
#[test]
fn fixture() {
    let vfs = VirtualFileSystem::copy(crate::parent_directory!().join("in"));

    crate::cli!()
        .arg("add")
        .arg("--registry")
        .arg("http://my-reg.jfrog.io/artifactory")
        .arg("my-repository/my-package@=1.0.0")
        .current_dir(vfs.root())
        .assert()
        .success()
        .stdout(include_str!("stdout.log"))
        .stderr(include_str!("stderr.log"));

    vfs.verify_against(crate::parent_directory!().join("out"));
}
