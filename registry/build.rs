use std::{env, path::Path};

use buffrs::package::PackageStore;

#[tokio::main]
async fn main() {
    let cwd = {
        let root = env!("CARGO_MANIFEST_DIR");

        let mut workspace_dir = Path::new(root);

        while !workspace_dir.ends_with("buffrs") {
            workspace_dir = workspace_dir
                .parent()
                .expect("no path ending in 'buffrs' found in {root}");
        }

        let dir = workspace_dir.join("registry");

        assert!(
            dir.is_dir(),
            "current directory not found in {}",
            workspace_dir.display()
        );

        dir
    };

    env::set_current_dir(&cwd).unwrap();

    let store = cwd.join(PackageStore::PROTO_VENDOR_PATH);

    dbg!(&store);

    let protos = PackageStore::open(&cwd)
        .await
        .unwrap()
        .collect(&store, true)
        .await;

    let includes = &[store];

    tonic_build::configure()
        .build_client(true)
        .build_server(true)
        .build_transport(true)
        .include_file("buffrs.rs")
        .compile(&protos, includes)
        .unwrap();
}
