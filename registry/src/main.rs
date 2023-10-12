// (c) Copyright 2023 Helsing GmbH. All rights reserved.
use buffrs_registry::{db::connect, config::Config};
use clap::Parser;

#[tokio::main]
async fn main() -> eyre::Result<()> {
    let config = Config::parse();
    tracing_subscriber::fmt::init();
    let db = connect(&config.database).await.unwrap();
    Ok(())
}
