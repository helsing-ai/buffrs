// (c) Copyright 2023 Helsing GmbH. All rights reserved.

use crate::{manifest::Dependency, package::Package};

mod artifactory;

pub use artifactory::{Artifactory, ArtifactoryConfig};

/// Environment variable used for token if keyring is unavailable
pub const REGISTRY_TOKEN_VAR: &str = "BUFFRS_REGISTRY_TOKEN";

/// A `buffrs` registry used for remote package management
#[async_trait::async_trait]
pub trait Registry {
    /// Downloads a package from the registry
    async fn download(&self, dependency: Dependency) -> eyre::Result<Package>;
    /// Publishs a package to the registry
    async fn publish(&self, package: Package, repository: String) -> eyre::Result<()>;
}

/// An enum containing all supported registries
pub enum RegistryType {
    Artifactory,
}
