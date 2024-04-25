use crate::VirtualFileSystem;

#[test]
fn fixture() {
    let vfs = VirtualFileSystem::empty();

    crate::cli!()
        .arg("init")
        .arg("--lib")
        .arg("some-lib")
        .current_dir(vfs.root())
        .assert()
        .success()
        .stdout(include_str!("stdout.log"))
        .stderr(include_str!("stderr.log"));

    vfs.verify_against(crate::parent_directory!().join("out"));
}
