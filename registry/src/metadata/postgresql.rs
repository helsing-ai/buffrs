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

use super::*;

use buffrs::package::PackageType;
use semver::Version;
use serde::{Deserialize, Serialize};
use sha3::{Digest, Sha3_256};
use sqlx::postgres::PgPoolOptions;
use sqlx::{PgPool, Pool, Postgres};
use std::fmt::Debug;
use std::str::FromStr;
use tracing::{error, info};

/// Pgsql MetadataStorage
pub struct PgsqlMetadataStorage {
    pool: Pool<Postgres>,
}

#[async_trait::async_trait]
impl TryFetch<PgsqlMetadataStorage> for PackageManifest {
    async fn try_fetch(
        package: PackageVersion,
        e: &PgsqlMetadataStorage,
    ) -> Result<PackageManifest, MetadataStorageError> {
        let Some(searched) = sqlx::query_as::<_, PgPackageVersionQuery>(SELECT_VERSION_QUERY)
            .bind(package.package.to_string())
            .bind(package.version.to_string())
            .fetch_optional(&e.pool)
            .await
            .map_err(|x| {
                error!("Error: SELECT_VERSION_QUERY {}", x);
                MetadataStorageError::Internal
            })?
        else {
            return Err(MetadataStorageError::PackageMissing(
                package.package.to_string(),
                Some(package.version.to_string()),
            ));
        };

        let manifest = PackageManifest {
            kind: searched.kind.into(),
            name: PackageName::from_str(&searched.name).map_err(|x| {
                error!(
                    "Error: {}, packageName: {} couldn't be mapped to a PackageName",
                    x, &searched.name
                );
                MetadataStorageError::Internal
            })?,
            version: Version::from_str(&searched.version).map_err(|x| {
                error!(
                    "Error: {}, version {} couldn't be mapped to a semver::Version",
                    x, &searched.version
                );
                MetadataStorageError::Internal
            })?,
            description: Some("unsupported".to_string()),
        };

        Ok(manifest)
    }
}

#[async_trait::async_trait]
impl FetchAllMatching<PgsqlMetadataStorage> for PackageManifest {
    async fn fetch_matching(
        package: PackageName,
        version: VersionReq,
        e: &PgsqlMetadataStorage,
    ) -> Result<Vec<PackageManifest>, MetadataStorageError> {
        let results = sqlx::query_as::<_, PgPackageVersionQuery>(SELECT_VERSION_COLLATE)
            .bind(package.to_string())
            .fetch_all(&e.pool)
            .await
            .map_err(|x| {
                error!("Error fetching versions: {}", x);
                MetadataStorageError::Internal
            })?;

        let mut manifests: Vec<PackageManifest> = vec![];
        for searched in results {
            let Ok(pkg_version) = Version::from_str(&searched.version) else {
                error!(
                    "Package {}, version: {} couldn't be interpreted as a version",
                    searched.name, searched.version
                );
                continue;
            };

            if !version.matches(&pkg_version) {
                continue;
            }

            let manifest = PackageManifest {
                kind: searched.kind.into(),
                name: PackageName::from_str(&searched.name).map_err(|x| {
                    error!(
                        "Error: {}, packageName: {} couldn't be mapped to a PackageName",
                        x, &searched.name
                    );
                    MetadataStorageError::Internal
                })?,
                version: Version::from_str(&searched.version).map_err(|x| {
                    error!(
                        "Error: {}, version {} couldn't be mapped to a semver::Version",
                        x, &searched.version
                    );
                    MetadataStorageError::Internal
                })?,
                description: Some("unsupported".to_string()),
            };

            manifests.push(manifest);
        }

        Ok(manifests)
    }
}

#[async_trait::async_trait]
impl Publish<PgsqlMetadataStorage> for PackageManifest {
    async fn publish(
        package: PackageManifest,
        e: &PgsqlMetadataStorage,
    ) -> Result<(), MetadataStorageError> {
        let package_lib = PgPackageType::from(package.kind);

        let mut tx = e.pool.begin().await.map_err(|x| {
            error!("Error starting transaction: {}", x);
            MetadataStorageError::Internal
        })?;
        // select or insert package
        let db_package = sqlx::query_as::<_, PgPackage>(SELECT_OR_INSERT_PACKAGE_QUERY)
            .bind(package.name.to_string())
            .bind(package_lib)
            .fetch_one(&mut *tx)
            .await
            .map_err(|_| MetadataStorageError::Internal)?;

        if sqlx::query_as::<_, PgPackageVersionQuery>(SELECT_VERSION_QUERY)
            .bind(package.name.to_string())
            .bind(package.version.to_string())
            .fetch_optional(&mut *tx)
            .await
            .map_err(|_| MetadataStorageError::Internal)?
            .is_some()
        {
            info!(
                "Duplicate Inserting package: {}, version: {}",
                package.name, package.version
            );
            return Err(MetadataStorageError::PackageDuplicate(
                package.name.to_string(),
                package.version.to_string(),
            ));
        };

        let version_hash = hash_string(package.version.to_string().as_str());

        let _query = sqlx::query(INSERT_VERSION_QUERY)
            .bind(db_package.id)
            .bind(package.version.to_string())
            .bind(version_hash)
            .execute(&mut *tx)
            .await
            .map_err(|x| {
                error!("Error: {}", x);
                MetadataStorageError::Internal
            })?;

        let commit_result = tx.commit().await;
        if let Err(err) = commit_result {
            error!("Error on commit: {}", err);
            return Err(MetadataStorageError::Internal);
        }

        Ok(())
    }
}

impl PgsqlMetadataStorage {
    /// Creates a PgsqlMetadataStorage from a given Pgsql Connection Pool
    pub fn new(pool: PgPool) -> Self {
        PgsqlMetadataStorage { pool }
    }

    /// Connects to the DB, migrate it and then starts the storage
    pub async fn connect(
        connection_string: &str,
        max_connections: u32,
    ) -> Result<PgsqlMetadataStorage, sqlx::Error> {
        let pool = PgPoolOptions::new()
            .max_connections(max_connections)
            .connect(connection_string)
            .await?;

        sqlx::migrate!().run(&pool).await?;

        Ok(PgsqlMetadataStorage { pool })
    }
}

impl Debug for PgsqlMetadataStorage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PgsqlMetadataStorage").finish()
    }
}

// PgPackage From Row
/// TODO, move to a Model
#[derive(sqlx::FromRow)]
struct PgPackage {
    id: i32,
}

const SELECT_OR_INSERT_PACKAGE_QUERY: &str = r#"
    WITH ins AS (
        INSERT INTO packages (name, type, created_at, updated_at)
        SELECT $1, $2, NOW(), NOW()
        WHERE NOT EXISTS (SELECT 1 FROM packages WHERE name = $1)
        RETURNING *
    )
    SELECT ins.id FROM ins
    UNION ALL
    SELECT packages.id FROM packages WHERE name = $1;
"#;

/// PgPackage Version
/// TODO, move to a Model & ORM style
#[derive(sqlx::FromRow, Debug)]
struct PgPackageVersionQuery {
    name: String,
    #[sqlx(rename = "type")]
    kind: PgPackageType,
    version: String,
}

const SELECT_VERSION_QUERY: &str = r#"
    SELECT p.id, p.name, p.type, v.version
    FROM versions v
    INNER JOIN packages p ON v.package_id = p.id
    WHERE p.name = $1 and v.version = $2;
"#;

const SELECT_VERSION_COLLATE: &str = r#"
    SELECT p.id, p.name, p.type, v.version
    FROM versions v
    INNER JOIN packages p ON v.package_id = p.id
    WHERE p.name = $1
    ORDER BY v.version COLLATE semver_col;
"#;

const INSERT_VERSION_QUERY: &str = r#"
    INSERT INTO versions (
        package_id,
        version,
        checksum,
        authors,
        description,
        keywords,
        documentation,
        homepage,
        license,
        repository,
        created_at,
        yanked_at
    )
    VALUES (
        $1, -- package_id
        $2, -- version
        $3,
        ARRAY['author1', 'author2'],   -- todo
        'description',   -- todo
        ARRAY['keyword1', 'keyword2'],  -- todo
        'documentation',  -- todo
        'homepage',   -- todo
        'license',   -- todo
        'repository',   -- todo
        CURRENT_TIMESTAMP,
        NULL
    );
"#;

fn hash_string(input: &str) -> String {
    let mut hasher = Sha3_256::new();
    hasher.update(input);
    let result = hasher.finalize();
    let str = format!("{:x}", result);
    str
}

/// Buffrs Package Types
#[derive(Copy, Clone, Debug, Serialize, Deserialize, PartialEq, Eq, sqlx::Type)]
#[sqlx(type_name = "package_type", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum PgPackageType {
    /// A library package
    Library,
    /// An api package
    Api,
}

impl From<PgPackageType> for PackageType {
    fn from(val: PgPackageType) -> Self {
        match val {
            PgPackageType::Library => PackageType::Lib,
            PgPackageType::Api => PackageType::Api,
        }
    }
}

impl From<PackageType> for PgPackageType {
    fn from(val: PackageType) -> Self {
        match val {
            PackageType::Lib => PgPackageType::Library,
            PackageType::Api => PgPackageType::Api,
        }
    }
}
