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

//! # Shared type definitions

use buffrs::package::{PackageName, PackageType};
use semver::Version;

/// Represents a Buffrs package version
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct PackageVersion {
    /// Package name
    pub package: PackageName,

    /// Package version
    pub version: Version,
}

impl PackageVersion {
    /// Determine the file name of a package.
    pub fn file_name(&self) -> String {
        let Self { package, version } = &self;
        format!("{package}_{version}.tar.gz")
    }
}

impl From<crate::proto::buffrs::package::Type> for PackageType {
    fn from(value: crate::proto::buffrs::package::Type) -> Self {
        match value {
            crate::proto::buffrs::package::Type::Library => PackageType::Lib,
            crate::proto::buffrs::package::Type::Api => PackageType::Api,
        }
    }
}
