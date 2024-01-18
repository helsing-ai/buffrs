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

//! # Buffrs Registry API

use crate::context::Context;
use crate::proto::buffrs::registry::registry_server::RegistryServer;
use eyre::Result;
use tonic::transport::Server;

mod grpc;

impl Context {
    /// Launch API
    pub async fn launch_api(&self) -> Result<()> {
        let ctx = self.clone();

        let server = Server::builder().add_service(RegistryServer::new(ctx));

        let listen_address = self.listen_address();
        tracing::info!("Starting server: {:?}", listen_address);

        server.serve(listen_address).await?;

        Ok(())
    }
}
