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

use std::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};

/// Package types
#[derive(
    Copy, Clone, Debug, Hash, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Default,
)]
#[serde(rename_all = "snake_case")]
pub enum PackageType {
    /// A library package containing primitive type definitions
    Lib,
    /// An api package containing message and service definition
    Api,
    /// An implementation package that implements an api or library
    ///
    /// Note: Implementation packages can't be published via Buffrs
    #[default]
    Impl,
}

impl PackageType {
    /// Whether this package type is publishable
    pub fn is_publishable(&self) -> bool {
        *self != Self::Impl
    }

    /// Whether this package type is compilable
    pub fn is_compilable(&self) -> bool {
        *self != Self::Impl
    }
}

impl FromStr for PackageType {
    type Err = serde_typename::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        serde_typename::from_str(s)
    }
}

impl fmt::Display for PackageType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match serde_typename::to_str(self) {
            Ok(value) => f.write_str(value),
            Err(_error) => unreachable!(),
        }
    }
}

#[test]
fn can_check_publishable() {
    assert!(PackageType::Lib.is_publishable());
    assert!(PackageType::Api.is_publishable());
    assert!(!PackageType::Impl.is_publishable());
}

#[test]
fn can_check_compilable() {
    assert!(PackageType::Lib.is_compilable());
    assert!(PackageType::Api.is_compilable());
    assert!(!PackageType::Impl.is_compilable());
}

#[test]
fn can_default_package_type() {
    assert_eq!(PackageType::default(), PackageType::Impl);
}

#[test]
fn can_parse_package_type() {
    let types = [PackageType::Lib, PackageType::Api, PackageType::Impl];
    for typ in &types {
        let string = typ.to_string();
        let parsed: PackageType = string.parse().unwrap();
        assert_eq!(parsed, *typ);
    }
}
