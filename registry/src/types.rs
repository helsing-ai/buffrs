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

use buffrs::package::PackageName;
use proptest::strategy::Strategy;
use semver::Version;
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq, Eq, Hash, test_strategy::Arbitrary)]
pub struct PackageVersion {
    #[filter(!#package.contains("/") && !#package.contains("."))]
    /// Package name
    pub package: Arc<str>,
    #[filter(!#version.contains("/") && !#version.contains("."))]
    /// Package version
    pub version: Arc<str>,
}

impl PackageVersion {
    /// Determine the file name of a package.
    pub fn file_name(&self) -> String {
        let Self { package, version } = &self;
        format!("{package}_{version}.tar.gz")
    }
}
