// Copyright 2023 Helsing GmbH
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use std::str::FromStr;

use crate::{
    context::Context,
    metadata::MetadataStorageError,
    proto::buffrs::package::Type,
    proto::buffrs::registry::{
        registry_server::Registry, DownloadRequest, DownloadResponse, PublishRequest,
        PublishResponse, VersionsRequest, VersionsResponse,
    },
    types::PackageVersion,
};
use async_trait::async_trait;
use buffrs::{manifest::PackageManifest, package::PackageName, package::PackageType};
use semver::Version;
use tonic::{Code, Request, Response, Status};
use tonic_types::{ErrorDetails, StatusExt};

/// Package's name characters limit, maybe should make it an option later
const PACKAGE_NAME_LIMIT: usize = 255;

#[async_trait]
impl Registry for Context {
    async fn publish(
        &self,
        request: Request<PublishRequest>,
    ) -> Result<Response<PublishResponse>, Status> {
        tracing::info!("Received request");
        let req: PublishRequest = request.into_inner();

        let package = req
            .package
            .ok_or(Status::invalid_argument("package wasn't set"))?;
        let metadata = package
            .metadata
            .ok_or(Status::invalid_argument("metadata wasn't set"))?;

        if metadata.name.len() > PACKAGE_NAME_LIMIT {
            let mut err_details = ErrorDetails::new();
            err_details.add_bad_request_violation(
                "name",
                format!("package's name exceeds limit: {}", PACKAGE_NAME_LIMIT),
            );

            // Generate error status
            let status = Status::with_error_details(
                Code::InvalidArgument,
                "request contains invalid arguments",
                err_details,
            );
            return Err(status);
        }

        let package_name = PackageName::from_str(metadata.name.as_str()).map_err(|error| {
            Status::invalid_argument(format!("Package name isn't correct, {:?}", error))
        })?;

        let version = Version::from_str(metadata.version.as_str())
            .map_err(|_| Status::invalid_argument("version isn't correct"))?;

        let package_version = &PackageVersion {
            package: package_name.clone(),
            version: version.clone(),
        };

        // need to add some more checks (version conflict, do we allow overriding?)
        let metadata_store = self.metadata_store();
        match metadata_store.get_version(package_version.clone()).await {
            Ok(_) => {
                tracing::info!(
                    "Package: {}, version: {} already exists, publish refused",
                    package_name,
                    version
                );
                return Err(Status::already_exists(
                    "Package already exist for this version",
                ));
            }
            Err(MetadataStorageError::PackageMissing { .. }) => {
                // on publish this is normal behavior to ignore
            }
            Err(_) => {
                return Err(Status::internal("error"));
            }
        }

        let storage = self.storage();

        let vec_ref = package.tgz.as_ref();
        let package_bytes: &[u8] = vec_ref;

        storage
            .package_put(package_version, package_bytes)
            .await
            .map_err(|_| Status::internal("Something went wrong on our side, sorry :("))?;

        let package_type = Type::try_from(metadata.r#type)
            .map(PackageType::from)
            .map_err(|_| Status::internal("couldn't map package type"))?;

        let package_manifest = PackageManifest {
            kind: package_type as PackageType,
            name: package_name.clone(),
            version: version.clone(),
            description: None,
        };

        match metadata_store.put_version(package_manifest).await {
            Ok(_) => {}
            Err(MetadataStorageError::PackageDuplicate(..)) => {
                return Err(Status::already_exists(
                    "Package already exist for this version",
                ));
            }
            Err(_) => {}
        };

        Ok(Response::new(PublishResponse {}))
    }

    async fn download(
        &self,
        request: Request<DownloadRequest>,
    ) -> Result<Response<DownloadResponse>, Status> {
        let _req: DownloadRequest = request.into_inner();
        todo!()
    }

    async fn versions(
        &self,
        request: Request<VersionsRequest>,
    ) -> Result<Response<VersionsResponse>, Status> {
        let _req: VersionsRequest = request.into_inner();
        // check from metadata
        // if not
        todo!()
    }
}
