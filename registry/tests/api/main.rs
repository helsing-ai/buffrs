use buffrs_registry::proto::buffrs::package::{Compressed, Package};
use buffrs_registry::proto::buffrs::registry::registry_client::RegistryClient;
use buffrs_registry::proto::buffrs::registry::PublishRequest;
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

#[ignore = "temporary"]
#[tokio::test]
async fn test_publish_registry() -> Result<(), Box<dyn std::error::Error>> {
    let mut client = RegistryClient::connect("http://localhost:4367").await?;

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
