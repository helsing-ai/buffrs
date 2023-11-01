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

/// Postgres metadata implementation.
///
/// This implements the [`Metadata`] trait using a postgres database, using the `sqlx` crate for
/// interactions with the database.
#[derive(Clone, Debug)]
pub struct Postgres {
    pool: PgPool,
}

impl Postgres {
    /// Connect to the database.
    pub async fn connect(url: &Url) -> Result<Self, Error> {
        let pool = PgPool::connect(url.as_str()).await?;
        Ok(Self { pool })
    }

    /// Migrate database.
    pub async fn migrate(&self) -> Result<(), Error> {
        sqlx::migrate!().run(&self.pool).await?;
        Ok(())
    }
}

#[async_trait]
impl Metadata for Postgres {
    async fn write(&self) -> Result<Box<dyn WriteHandle>, BoxError> {
        Ok(Box::new(self.pool.begin().await.map_err(|e| Box::new(e))?))
    }

    async fn read(&self) -> Result<Box<dyn ReadHandle>, BoxError> {
        Ok(Box::new(
            self.pool.acquire().await.map_err(|e| Box::new(e))?,
        ))
    }
}

#[async_trait]
impl<T: DerefMut<Target = PgConnection> + Send + Sync> ReadHandle for T {
    async fn user_lookup(&mut self, handle: &str) -> User {
        query_as("SELECT * FROM users WHERE handle = $1")
            .bind(handle)
            .fetch_one(self.deref_mut())
            .await
            .unwrap()
    }

    async fn user_info(&mut self, user: &str) -> String {
        todo!()
    }

    async fn package_metadata(&mut self, package: &str) -> String {
        todo!()
    }

    async fn package_version(&mut self, package: &str) -> Vec<String> {
        todo!()
    }
}

#[async_trait]
impl WriteHandle for Transaction<'static, sqlx::Postgres> {
    async fn user_create(&mut self, user: &str) -> Result<(), ()> {
        let result = query("INSERT INTO users(handle) VALUES ($1) RETURNING (id)")
            .bind(user)
            .fetch_one(&mut **self)
            .await
            .unwrap();
        Ok(())
    }

    async fn user_token_create(&mut self, user: &str, token: &str) -> Result<(), ()> {
        todo!()
    }

    async fn user_token_delete(&mut self, user: &str, token: &str) -> Result<(), ()> {
        todo!()
    }

    async fn package_create(&mut self, package: &str) -> Result<(), ()> {
        todo!()
    }

    async fn package_version_create(
        &mut self,
        package: &str,
        version: &str,
        signature: &str,
    ) -> Result<(), ()> {
        todo!()
    }

    async fn package_version_yank(
        &mut self,
        package: &str,
        version: &str,
        signature: &str,
    ) -> Result<(), ()> {
        todo!()
    }

    async fn package_version_download(
        &mut self,
        package: &str,
        version: &str,
        count: u64,
    ) -> Result<(), ()> {
        todo!()
    }

    async fn commit(self) -> Result<(), ()> {
        Transaction::commit(self).await.unwrap();
        Ok(())
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
}
