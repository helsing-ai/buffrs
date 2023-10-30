use crate::VirtualFileSystem;

#[test]
#[ignore = "sha mismatch, tar files should be compared by contents!"]
fn fixture() {
    let vfs = VirtualFileSystem::copy(crate::parent_directory!().join("in"));

    crate::cli!()
        .arg("package")
        .current_dir(vfs.root())
        .assert()
        .success()
        .stdout(include_str!("stdout.log"))
        .stderr(include_str!("stderr.log"))
        .code(0);

    vfs.verify_against(crate::parent_directory!().join("out"));
}
