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

use crate::{
    context::Context,
    proto::buffrs::registry::{
        registry_server::Registry, DownloadRequest, DownloadResponse, PublishRequest,
        PublishResponse, VersionsRequest, VersionsResponse,
    },
};
use async_trait::async_trait;
use tonic::{Request, Response, Status};

#[async_trait]
impl Registry for Context {
    async fn publish(
        &self,
        _request: Request<PublishRequest>,
    ) -> Result<Response<PublishResponse>, Status> {
        todo!()
    }

    async fn download(
        &self,
        _request: Request<DownloadRequest>,
    ) -> Result<Response<DownloadResponse>, Status> {
        todo!()
    }

    async fn versions(
        &self,
        _request: Request<VersionsRequest>,
    ) -> Result<Response<VersionsResponse>, Status> {
        todo!()
    }
}
