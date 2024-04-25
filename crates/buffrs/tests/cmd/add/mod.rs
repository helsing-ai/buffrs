use crate::VirtualFileSystem;

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
