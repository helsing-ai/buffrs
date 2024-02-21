use std::path::Path;

use buffrs::package::PackageStore;

#[tokio::main]
async fn main() {
    let store = Path::new(PackageStore::PROTO_VENDOR_PATH);

    let protos = PackageStore::current()
        .await
        .unwrap()
        .collect(store, true)
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
