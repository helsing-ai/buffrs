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

use serde::{Deserialize, Serialize};
use strum::{Display, EnumString};

/// Package types
#[derive(
    Copy,
    Clone,
    Debug,
    Hash,
    Serialize,
    Deserialize,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    EnumString,
    Display,
)]
#[serde(rename_all = "snake_case")]
pub enum PackageType {
    /// A library package containing primitive type definitions
    Lib,
    /// An api package containing message and service definition
    Api,
}

impl TryFrom<i32> for PackageType {
    type Error = &'static str;

    fn try_from(value: i32) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(PackageType::Lib),
            2 => Ok(PackageType::Api),
            _ => Err("Invalid value, check `PackageType` potential values"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn can_parse_package_type() {
        let types = [PackageType::Lib, PackageType::Api];
        for typ in &types {
            let string = typ.to_string();
            let parsed: PackageType = string.parse().unwrap();
            assert_eq!(parsed, *typ);
        }
    }
}
