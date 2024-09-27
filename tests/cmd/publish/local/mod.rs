use crate::VirtualFileSystem;

#[test]
fn fixture() {
    let vfs = VirtualFileSystem::copy(crate::parent_directory!().join("in"));

    crate::cli!()
        .arg("publish")
        .arg("--registry")
        .arg("https://localhost:54321/fake-uri")
        .arg("--repository")
        .arg("my-repository")
        .current_dir(vfs.root())
        .assert()
        .failure()
        .stdout(include_str!("stdout.log"))
        .stderr(include_str!("stderr.log"));

    vfs.verify_against(crate::parent_directory!().join("out"));
}
