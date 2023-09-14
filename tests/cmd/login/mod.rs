use crate::VirtualFileSystem;

#[test]
fn fixture() {
    let vfs = VirtualFileSystem::empty().with_virtual_home();

    crate::cli!()
        .arg("login")
        .arg("--url")
        .arg("https://org.jfrog.io/artifactory")
        .current_dir(vfs.root())
        .write_stdin("some-token")
        .assert()
        .success()
        .stdout(include_str!("stdout.log"))
        .stderr(include_str!("stderr.log"))
        .code(0);

    vfs.verify_against(crate::parent_directory!().join("out"));
}
