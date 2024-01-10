use buffrs_registry::context::Context;
use buffrs_registry::metadata::memory::InMemoryMetadataStorage;
use buffrs_registry::proto::buffrs::package::{Compressed, Package};

use buffrs_registry::proto::buffrs::registry::registry_server::Registry;
use buffrs_registry::proto::buffrs::registry::PublishRequest;
use buffrs_registry::storage;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::Path;
use std::sync::Arc;
use tonic::Code;

pub fn create_publish_request_sample() -> PublishRequest {
    PublishRequest {
        package: Some(Compressed {
            metadata: Some(Package {
                name: "testing".to_string(),
                version: "1.0.0".to_string(),
                r#type: 0,
            }),
            tgz: vec![0, 0, 0],
        }),
    }
}

#[tokio::test]
async fn test_publish_registry() -> Result<(), Box<dyn std::error::Error>> {
    let path = Path::new("/tmp");
    let storage = Arc::new(storage::Filesystem::new(path));
    let metadata = Arc::new(InMemoryMetadataStorage::new());
    let socket = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 0)), 1111);
    let fake_server = Context::new(storage, metadata, socket);

    // 1. Insert a package and expect it to be successful
    {
        let req = tonic::Request::new(create_publish_request_sample());
        let _res = fake_server.publish(req).await?;
        println!(":: Package Publish OK");
    }

    // 2. Insert the same package for a duplicate check
    {
        // duplicate check
        let req = tonic::Request::new(create_publish_request_sample());
        let Err(res) = fake_server
            .publish(req)
            .await
            .map_err(|status| status.code())
        else {
            panic!("Publish duplicate")
        };
        assert_eq!(res, Code::AlreadyExists);
        println!(":: Package Forbid Duplicate OK");
    }

    Ok(())
}
