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

pub use super::*;
pub use sqlx::{pool::PoolConnection, Error, PgConnection, PgPool, Transaction};
use std::ops::DerefMut;

/// Postgres-backed metadata store
///
/// This implements a [`Metadata`] store using a Postgres database. The [`WriteHandle`] is
/// implemented as a transaction, while the [`ReadHandle`] is implementated as a regular
/// connection.
///
/// Internally, it uses the `sqlx` crate for interactions with the database.
#[derive(Clone, Debug)]
pub struct Postgres {
    pool: PgPool,
}

impl Postgres {
    /// Connect to the database.
    ///
    /// This expects a [`Url`] in the shape of `postgres://username:password@hostname`. See the
    /// documentation of the `sqlx` crate for information on what else this may contain.
    pub async fn connect(url: &Url) -> Result<Self, Error> {
        let pool = PgPool::connect(url.as_str()).await?;
        Ok(Self { pool })
    }

    /// Migrate database.
    ///
    /// This applies the migrations stored in the repository to the connected database. This is not
    /// done automatically on startup, but must rather be performed explicitly.
    pub async fn migrate(&self) -> Result<(), Error> {
        sqlx::migrate!().run(&self.pool).await?;
        Ok(())
    }
}

#[async_trait]
impl Metadata for Postgres {
    async fn write(&self) -> Result<Box<dyn WriteHandle>, SharedError> {
        Ok(Box::new(
            self.pool
                .begin()
                .await
                .map_err(|e| Arc::new(e) as SharedError)?,
        ))
    }

    async fn read(&self) -> Result<Box<dyn ReadHandle>, SharedError> {
        Ok(Box::new(
            self.pool
                .begin()
                .await
                .map_err(|e| Arc::new(e) as SharedError)?,
        ))
    }
}

#[async_trait]
impl<T: DerefMut<Target = PgConnection> + Send + Sync> ReadHandle for T {
    async fn user_info(&mut self, handle: &Handle) -> Result<UserInfo, ReadError> {
        let result = query_as("SELECT * FROM users_active WHERE handle = $1")
            .bind(&**handle)
            .fetch_one(self.deref_mut())
            .await
            .unwrap();
        Ok(result)
    }

    async fn token_check(&mut self, user: &Token) -> Result<TokenInfo, ReadError> {
        todo!()
    }

    async fn token_info(&mut self, user: &TokenPrefix) -> Result<TokenInfo, ReadError> {
        todo!()
    }

    async fn package_metadata(&mut self, package: &str) -> Result<String, ReadError> {
        todo!()
    }

    async fn package_versions(&mut self, package: &str) -> Result<Vec<String>, ReadError> {
        todo!()
    }
}

#[async_trait]
impl WriteHandle for Transaction<'static, sqlx::Postgres> {
    async fn user_create(&mut self, user: &Handle) -> Result<(), WriteError> {
        let result = query("INSERT INTO users(handle) VALUES ($1) RETURNING *")
            .bind(&**user)
            .fetch_one(&mut **self)
            .await
            .unwrap();
        Ok(())
    }

    async fn user_token_create(
        &mut self,
        user: &Handle,
        token: &Token,
        permissions: &TokenPermissions,
    ) -> Result<(), WriteError> {
        query(
            "INSERT INTO user_tokens(user, prefix, hash, allow_publish, allow_update, allow_yank)
            VALUES (
                (SELECT id FROM users WHERE handle = $1),
                $2,
                $3,
                $4,
                $5,
                $6
            )",
        )
        .bind(&**user)
        .bind(&**token)
        .bind(&**token)
        .bind(permissions.allow_publish)
        .bind(permissions.allow_update)
        .bind(permissions.allow_yank)
        .execute(&mut **self)
        .await
        .unwrap();
        Ok(())
    }

    async fn user_token_delete(
        &mut self,
        user: &Handle,
        token: &TokenPrefix,
    ) -> Result<(), WriteError> {
        todo!()
    }

    async fn package_create(&mut self, package: &str) -> Result<(), WriteError> {
        todo!()
    }

    async fn package_version_create(
        &mut self,
        package: &str,
        version: &str,
        signature: &str,
    ) -> Result<(), WriteError> {
        todo!()
    }

    async fn package_version_yank(
        &mut self,
        package: &str,
        version: &str,
        signature: &str,
    ) -> Result<(), WriteError> {
        todo!()
    }

    async fn package_version_download(
        &mut self,
        package: &str,
        version: &str,
        count: u64,
    ) -> Result<(), WriteError> {
        todo!()
    }

    async fn commit(self: Box<Self>) -> Result<(), WriteError> {
        Transaction::commit(*self).await.unwrap();
        Ok(())
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::metadata::tests::*;
    use rand::{thread_rng, Rng};
    use sqlx::query;

    /// Generate random name for a bucket.
    fn random_database(prefix: &str) -> String {
        let mut rng = thread_rng();
        let name: String = (0..32).map(|_| rng.gen_range('a'..'z')).collect();
        format!("{prefix}_{name}")
    }

    /// Generate temporary new Postgres instance
    pub async fn temp_postgres() -> (Postgres, Cleanup) {
        // connect to root pool
        let root = PgPool::connect("postgres://buffrs:buffrs@localhost")
            .await
            .unwrap();

        // create random database
        let dbname = random_database("temp");
        println!("Creating temporary database {dbname}");
        query(&format!("CREATE DATABASE {dbname}"))
            .execute(&root)
            .await
            .unwrap();

        // connect to database
        let url = format!("postgres://buffrs:buffrs@localhost/{dbname}")
            .parse()
            .unwrap();
        let postgres = Postgres::connect(&url).await.unwrap();

        // migrate
        postgres.migrate().await.unwrap();

        // cleanup
        let pool = postgres.pool.clone();
        let cleanup = async move {
            pool.close().await;

            println!("Deleting temporary database {dbname}");
            let result = query(&format!("DROP DATABASE {dbname}"))
                .execute(&root)
                .await;

            // Sometimes, deleting does not work without an obvious reason. We don't actually care
            // if deleting works or not, we only try to do it to avoid you ending up with millions
            // of temporary databases. Here, we just log the error if one occurs and move on.
            if let Err(error) = &result {
                println!("Error deleting {dbname}: {error}");
            }
        };

        (postgres, Box::pin(cleanup))
    }

    #[proptest(async = "tokio", cases = 10)]
    async fn can_write_user(name: Handle) {
        with(temp_postgres, |postgres| async move {
            let mut writer = postgres.write().await.unwrap();
            writer.user_create(&name).await.unwrap();
            writer.commit().await.unwrap();
        })
        .await;
    }

    #[proptest(async = "tokio", cases = 10)]
    async fn can_read_user(name: Handle) {
        with(temp_postgres, |postgres| async move {
            let mut writer = postgres.write().await.unwrap();
            writer.user_create(&name).await.unwrap();
            writer.commit().await.unwrap();
        })
        .await;
    }
}
