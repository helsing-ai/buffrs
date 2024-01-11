use buffrs_registry::context::Context;
use buffrs_registry::metadata::memory::InMemoryMetadataStorage;
use buffrs_registry::proto::buffrs::package::{Compressed, Package};
use buffrs_registry::proto::buffrs::registry::registry_client::RegistryClient;
use buffrs_registry::proto::buffrs::registry::registry_server::{Registry, RegistryServer};
use buffrs_registry::proto::buffrs::registry::PublishRequest;
use buffrs_registry::storage;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::Path;
use std::str::FromStr;
use std::sync::Arc;
use tokio::io::DuplexStream;
use tonic::codegen::tokio_stream;
use tonic::transport::{Server, Channel};
use tonic::Code;
use tonic::{
    transport::{Endpoint, Uri},
    Request, Response, Status,
};
use tower::service_fn;

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

pub async fn basic_setup() -> Result<RegistryClient<Channel>, Box<dyn std::error::Error>>
{
    let (client, server) = tokio::io::duplex(1024);

    let path = Path::new("/tmp");
    let storage = Arc::new(storage::Filesystem::new(path));
    let metadata = Arc::new(InMemoryMetadataStorage::new());

    let url = "0.0.0.0:0";

    // this is needs to be removed once Context got cleaned up
    let socket = SocketAddr::from_str(url).expect("Shouldn't happen");
    let context = Context::new(storage, metadata, socket);

    let _handle = tokio::spawn(async move {
        Server::builder()
            .add_service(RegistryServer::new(context))
            .serve_with_incoming(tokio_stream::once(Ok::<_, std::io::Error>(server)))
            .await
    });

    // Move client to an option so we can _move_ the inner value
    // on the first attempt to connect. All other attempts will fail.
    let mut client = Some(client);
    let channel = Endpoint::try_from(format!("http://{}", url))?
        .connect_with_connector(service_fn(move |_: Uri| {
            let client = client.take();

            async move {
                if let Some(client) = client {
                    Ok(client)
                } else {
                    Err(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        "Client already taken",
                    ))
                }
            }
        }))
        .await?;

    return Ok(RegistryClient::new(channel));
}

#[tokio::test]
async fn test_publish_registry() -> Result<(), Box<dyn std::error::Error>> {
    let mut client = basic_setup().await?;

    // 1. Insert a package and expect it to be successful
    {
        let req = tonic::Request::new(create_publish_request_sample());
        let _res = client.publish(req).await?;
        println!(":: Package Publish OK");
    }

    // 2. Insert the same package for a duplicate check
    {
        // duplicate check
        let req = tonic::Request::new(create_publish_request_sample());
        let Err(res) = client.publish(req).await.map_err(|status| status.code()) else {
            panic!("Publish duplicate")
        };
        assert_eq!(res, Code::AlreadyExists);
        println!(":: Package Forbid Duplicate OK");
    }

    Ok(())
}
