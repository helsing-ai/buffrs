use crate::{VirtualFileSystem, with_test_registry};

#[test]
fn fixture() {
    with_test_registry(|url| {
        let vfs = VirtualFileSystem::copy(crate::parent_directory!().join("in"));

        crate::cli!()
            .arg("publish")
            .arg("--registry")
            .arg(url)
            .arg("--repository")
            .arg("my-repository")
            .current_dir(vfs.root())
            .assert()
            .success()
            .stdout(include_str!("stdout.log"))
            .stderr(include_str!("stderr.log"));

        vfs.verify_against(crate::parent_directory!().join("out"));
    });
}
