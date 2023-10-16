use super::*;
use proptest::prelude::*;
use sqlx::testing::{TestArgs, TestFn};

#[sqlx::test]
async fn can_migrate(pool: Pool) {}

#[sqlx::test]
async fn can_create_user(pool: Pool) {
    let mut conn = pool.acquire().await.unwrap();
    let user = "abc";
    conn.user_create(user).await.unwrap();
    let user = conn.user_lookup(user).await;
}

#[sqlx::test]
async fn can_create_token(pool: Pool) {
    let mut conn = pool.acquire().await.unwrap();
    let user = "abc";
    conn.user_create(user).await.unwrap();
    let token = "asda";
    conn.user_token_create(user, token).await.unwrap();
}
