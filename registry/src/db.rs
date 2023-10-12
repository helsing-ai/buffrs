// (c) Copyright 2023 Helsing GmbH. All rights reserved.
use sqlx::PgPool;

pub async fn connect(string: &str) -> eyre::Result<PgPool> {
    let pool = PgPool::connect(string).await?;

    sqlx::migrate!()
        .run(&pool)
        .await?;

    Ok(pool)
}
