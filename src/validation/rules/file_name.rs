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
use std::path::PathBuf;

/// Ensure that file names match package names.
///
/// For example, a package named `physics` should be stored in `physics.proto`. A package named
/// `physics.rotation` should be stored in `physics/rotation.proto`.
#[derive(Debug, Clone, Default)]
pub struct FileName;

fn file_name(package_name: &str) -> PathBuf {
    format!("{}.proto", package_name.replace('.', "/")).into()
}

impl Rule for FileName {
    fn rule_info(&self) -> &'static str {
        "making sure file names matches package names"
    }

    fn check_package(&mut self, package: &Package) -> Violations {
        let candidate = file_name(&package.name);
        if candidate != package.file {
            let message = violation::Message {
                message: format!(
                    "file name should be {candidate:?} but is {:?}",
                    package.file
                ),
                help: "Try to rename the file to align with the package name.".into(),
            };

            return vec![self.to_violation(message)];
        }

        Violations::default()
    }
}

#[test]
fn file_name_package() {
    assert_eq!(file_name("physics"), PathBuf::from("physics.proto"));
    assert_eq!(
        file_name("physics.rotation"),
        PathBuf::from("physics/rotation.proto")
    );
}

#[test]
fn correct_file_name() {
    let package = Package {
        name: "my_package".into(),
        file: "my_package.proto".into(),
        entities: Default::default(),
    };
    let mut rule = FileName;
    assert!(rule.check_package(&package).is_empty());
}

#[test]
fn correct_file_name_subpackage() {
    let package = Package {
        name: "my_package.subpackage".into(),
        file: "my_package/subpackage.proto".into(),
        entities: Default::default(),
    };
    let mut rule = FileName;
    assert!(rule.check_package(&package).is_empty());
}

#[test]
fn incorrect_file_name() {
    let package = Package {
        name: "my_package".into(),
        file: "my_package_other.proto".into(),
        entities: Default::default(),
    };
    let mut rule = FileName;
    assert_eq!(
        rule.check_package(&package),
        vec![Violation {
            rule: "FileName".into(),
            level: Level::Error,
            location: Default::default(),
            info: rule.rule_info().into(),
            message: violation::Message {
                message:
                    r#"file name should be "my_package.proto" but is "my_package_other.proto""#
                        .into(),
                help: "Try to rename the file to align with the package name.".into(),
            }
        }]
    );
}

#[test]
fn incorrect_file_name_subpackage() {
    let package = Package {
        name: "my_package.subpackage".into(),
        file: "my_package/my_subpackage.proto".into(),
        entities: Default::default(),
    };
    let mut rule = FileName;
    assert_eq!(
        rule.check_package(&package),
        vec![Violation {
            rule: "FileName".into(),
            level: Level::Error,
            location: Default::default(),
            info: rule.rule_info().into(),
            message: violation::Message {
                message: r#"file name should be "my_package/subpackage.proto" but is "my_package/my_subpackage.proto""#.into(),
                help: "Try to rename the file to align with the package name.".into(),
            }
        }]
    );
}
