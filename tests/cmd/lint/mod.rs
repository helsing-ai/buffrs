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

#[test]
fn exits_with_failure_when_a_rule_is_violated() {
    let vfs = VirtualFileSystem::empty();
    let root = vfs.root();

    crate::cli!()
        .args(["init", "--lib", "my-protos"])
        .current_dir(&root)
        .assert()
        .success();

    std::fs::write(
        root.join("proto/my_protos.proto"),
        "syntax = \"proto3\";\npackage wrongpkg;\nmessage Foo { string bar = 1; }\n",
    )
    .unwrap();

    let output = crate::cli!()
        .arg("lint")
        .current_dir(&root)
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&output.get_output().stderr);
    assert!(
        stderr.contains("wrongpkg"),
        "expected the package-name violation in stderr, got:\n{stderr}"
    );
    assert!(
        stderr.contains("found 1 rule violation(s)"),
        "expected a failing lint summary in stderr, got:\n{stderr}"
    );
}

#[test]
fn allow_warnings_still_fails_on_errors() {
    let vfs = VirtualFileSystem::empty();
    let root = vfs.root();

    crate::cli!()
        .args(["init", "--lib", "my-protos"])
        .current_dir(&root)
        .assert()
        .success();

    std::fs::write(
        root.join("proto/my_protos.proto"),
        "syntax = \"proto3\";\npackage wrongpkg;\nmessage Foo { string bar = 1; }\n",
    )
    .unwrap();

    crate::cli!()
        .args(["lint", "--allow", "warnings"])
        .current_dir(&root)
        .assert()
        .failure();
}

#[test]
fn deny_warnings_keeps_a_clean_package_successful() {
    let vfs = VirtualFileSystem::copy(crate::parent_directory!().join("in"));

    crate::cli!()
        .args(["lint", "--deny", "warnings"])
        .current_dir(vfs.root())
        .assert()
        .success()
        .stdout(include_str!("stdout.log"))
        .stderr(include_str!("stderr.log"));
}
