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

use crate::{manifest::Dependency, package::Package};

mod artifactory;
#[cfg(test)]
mod local;

pub use artifactory::{Artifactory, ArtifactoryConfig};

/// A `buffrs` registry used for remote package management
#[async_trait::async_trait]
pub trait Registry {
    /// Downloads a package from the registry
    async fn download(&self, dependency: Dependency) -> eyre::Result<Package>;
    /// Publishes a package to the registry
    async fn publish(&self, package: Package, repository: String) -> eyre::Result<()>;
}

/// An enum containing all supported registries
pub enum RegistryType {
    Artifactory,
}
