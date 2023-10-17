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

use clap::Parser;
use std::net::SocketAddr;
use url::Url;

#[derive(Parser, Clone, Debug)]
pub struct Options {
    /// Address to listen to for incoming connections.
    #[clap(long, short, env, default_value = "0.0.0.0:4367")]
    pub listen: SocketAddr,

    /// URL of Postgres database to connect to.
    #[clap(long, short, env)]
    #[cfg_attr(dev, clap(default_value = "postgres://buffrs:buffrs@localhost"))]
    pub database: Url,
}
