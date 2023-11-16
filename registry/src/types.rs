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

use proptest::prelude::*;

use buffrs::package::PackageName;
use semver::{BuildMetadata, Prerelease, Version};
use test_strategy::Arbitrary;

prop_compose! {
    fn package_name()(name in "[a-z][a-z0-9-]{0,127}") -> PackageName {
        name.try_into().unwrap()
    }
}

prop_compose! {
    fn package_version()(major: u64, minor: u64, patch: u64) -> Version {
        Version {
            minor,
            major,
            patch,
            pre: Prerelease::EMPTY,
            build: BuildMetadata::EMPTY,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Arbitrary)]
pub struct PackageVersion {
    /// Package name
    #[strategy(package_name())]
    pub package: PackageName,

    #[strategy(package_version())]
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
