// (c) Copyright 2023 Helsing GmbH. All rights reserved.
use clap::Parser;
use std::net::SocketAddr;

#[derive(Parser, Clone, Debug)]
pub struct Config {
    /// Address to listen to for incoming connections.
    #[clap(long, short, env, default_value = "0.0.0.0:8000")]
    pub listen: SocketAddr,

    /// URL of Postgres database to connect to.
    #[clap(long, short, env, default_value = "postgres://buffrs:buffrs@localhost")]
    pub database: String,
}
