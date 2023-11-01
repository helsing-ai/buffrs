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
pub use sqlx::{Error, PgConnection, PgPool};

/// Connect to the database.
pub async fn connect(url: &Url) -> Result<PgPool, Error> {
    let pool = PgPool::connect(url.as_str()).await?;
    Ok(pool)
}

/// Migrate database.
pub async fn migrate(pool: &PgPool) -> Result<(), Error> {
    sqlx::migrate!().run(pool).await?;
    Ok(())
}

#[async_trait]
impl Pool for PgPool {
    async fn get(&self) -> Result<Box<dyn Database>, ()> {
        todo!()
    }

    async fn begin(&self) -> Result<Box<dyn Transaction>, ()> {
        todo!()
    }
}

#[async_trait]
impl Database for PgConnection {
    async fn user_lookup(&mut self, handle: &str) -> User {
        query_as("SELECT * FROM users WHERE handle = $1")
            .bind(handle)
            .fetch_one(self)
            .await
            .unwrap()
    }

    async fn user_create(&mut self, user: &str) -> Result<(), ()> {
        let result = query("INSERT INTO users(handle) VALUES ($1) RETURNING (id)")
            .bind(user)
            .fetch_one(self)
            .await
            .unwrap();
        Ok(())
    }

    async fn user_token_auth(&mut self, token: &str) -> Result<(), ()> {
        query("SELECT 1").bind(token).fetch_one(self).await.unwrap();
        Ok(())
    }

    async fn user_cert_create(&mut self, user: &str, token: &str) -> Result<(), ()> {
        query(
            "INSERT INTO user_tokens(user_id, token)
            VALUES (
                (SELECT id FROM users WHERE handle = $1),
                $2
            )",
        )
        .bind(user)
        .bind(token)
        .execute(self)
        .await
        .unwrap();
        Ok(())
    }

    async fn user_cert_list(&mut self, user: &str) -> Result<Vec<String>, ()> {
        todo!()
    }

    async fn user_cert_delete(&mut self, user: &str, token: &str) -> Result<(), ()> {
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
}
