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

/// Ensure that the protobuf package names match the buffrs package name.
#[derive(Debug, Clone)]
pub struct PackageName {
    /// Package name to enforce.
    name: crate::package::PackageName,
}

impl PackageName {
    /// Create new checker for this rule.
    pub fn new(name: crate::package::PackageName) -> Self {
        Self { name }
    }
}

impl Rule for PackageName {
    fn rule_info(&self) -> &'static str {
        "Make sure that the protobuf package name matches the buffer package name."
    }

    fn check_package(&mut self, package: &Package) -> Violations {
        let transposed = self.name.to_string().replace('-', "_");

        if !is_prefix(&transposed, &package.name) {
            let message = violation::Message {
                message: format!("package name is {} but should have {} prefix", package.name, transposed),
                help: "Make sure the file name matches the package. For example, a package with the name `package.subpackage` should be stored in `proto/package/subpackage.proto`.".into(),
            };

            return vec![self.to_violation(message)];
        }

        Violations::default()
    }
}

fn is_prefix(prefix: &str, package: &str) -> bool {
    prefix
        .replace('-', "_")
        .split('.')
        .zip(package.split('.'))
        .all(|(a, b)| a == b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn can_check_prefix() {
        // any value is a prefix of itself
        assert!(is_prefix("abc", "abc"));

        assert!(is_prefix("abc", "abc.def"));
        assert!(is_prefix("abc", "abc.def.ghi"));
    }

    #[test]
    fn can_fail_wrong_prefix() {
        assert!(!is_prefix("abc", "def"));
        assert!(!is_prefix("abc", "abcdef"));
        assert!(!is_prefix("abc", ""));
        assert!(!is_prefix("abc", "ab"));
    }

    #[test]
    fn correct_package_name() {
        let package = Package {
            name: "my_package".into(),
            files: vec!["ignored.proto".into()],
            entities: Default::default(),
        };
        let mut rule = PackageName::new("my-package".parse().unwrap());
        assert!(rule.check_package(&package).is_empty());
    }

    #[test]
    fn correct_package_name_submodule() {
        let package = Package {
            name: "my_package.submodule".into(),
            files: vec!["ignored.proto".into()],
            entities: Default::default(),
        };
        let mut rule = PackageName::new("my-package".parse().unwrap());
        assert!(rule.check_package(&package).is_empty());
    }

    #[test]
    fn correct_case_transformation() {
        let package = Package {
            name: "my_package.submodule".into(),
            files: vec!["ignored.proto".into()],
            entities: Default::default(),
        };
        let mut rule = PackageName::new("my-package".parse().unwrap());
        assert!(rule.check_package(&package).is_empty());
    }

    #[test]
    fn incorrect_package_name() {
        let package = Package {
            name: "my_package_other".into(),
            files: vec!["ignored.proto".into()],
            entities: Default::default(),
        };
        let mut rule = PackageName::new("my-package".parse().unwrap());
        assert_eq!(
            rule.check_package(&package),
            vec![Violation {
                rule: "PackageName".into(),
                level: Level::Error,
                location: Default::default(),
                info: rule.rule_info().into(),
                message: violation::Message {
                    message: "package name is my_package_other but should have my_package prefix".into(),
                    help: "Make sure the file name matches the package. For example, a package with the name `package.subpackage` should be stored in `proto/package/subpackage.proto`.".into(),
                }
            }]
        );
    }
}
