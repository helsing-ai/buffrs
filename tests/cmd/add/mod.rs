use crate::VirtualFileSystem;

#[test]
fn fixture() {
    let vfs = VirtualFileSystem::copy(crate::test_input!());

    crate::cli()
        .arg("add")
        .arg("my-repository/my-package@=1.0.0")
        .current_dir(vfs.root())
        .assert()
        .success()
        .stdout(include_str!("stdout.log"))
        .stderr(include_str!("stderr.log"))
        .code(0);

    vfs.verify_against(crate::test_output!());
}
