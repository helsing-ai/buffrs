use crate::VirtualFileSystem;

#[test]
fn fixture() {
    let vfs = VirtualFileSystem::copy(crate::parent_directory!().join("in"));

    crate::cli!()
        .arg("lint")
        .current_dir(vfs.root())
        .assert()
        .success()
        .stdout(include_str!("stdout.log"))
        .stderr(include_str!("stderr.log"));
}
