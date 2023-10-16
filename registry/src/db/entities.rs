use sqlx::FromRow;

#[derive(Clone, Debug, FromRow)]
pub struct User {
    pub id: i64,
    pub token: String,
}

#[derive(Clone, Debug, FromRow)]
pub struct Package {}
