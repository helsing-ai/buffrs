use sqlx::FromRow;

#[derive(Clone, Debug, FromRow)]
pub struct User {
    pub id: i32,
    pub handle: String,
}

#[derive(Clone, Debug, FromRow)]
pub struct Package {}
