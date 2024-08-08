use super::*;

#[derive(Debug, Clone, Copy)]
pub struct PackageHierarchy;

impl Rule for PackageHierarchy {
    fn rule_info(&self) -> &'static str {
        "Ensure declared package hierarchy is mirrored in folder structure."
    }

    fn check_package(&mut self, package: &Package) -> Violations {
        let expected_components = package
            .name
            .split('.')
            .skip(1) // first part is the root package name, which is not expected to be a subfolder of proto
            .collect::<Vec<_>>();

        let expected_path = {
            let mut ret: String = "proto".into();
            for part in &expected_components {
                ret.push('/');
                ret.push_str(part);
            }
            ret
        };

        if expected_components.is_empty() {
            return vec![];
        }

        let expected_components = {
            let mut ret = expected_components;
            ret.reverse();

            ret
        };

        let violations = package
            .files
            .iter()
            .filter_map(|path| {
                if let Some(parent) = path.parent() {
                    let components = parent
                        .components()
                        .map(|c| String::from(c.as_os_str().to_string_lossy()))
                        .rev(); // work our way up

                    for (expected, component) in expected_components.iter().zip(components) {
                        if component != *expected {
                            return Some(self.to_violation(violation::Message {
                                message: format!("expected file {} to live in {}.", path.to_string_lossy(), &expected_path),
                                help: "Package names should be mirrored in folder structure, eg mypackage.subpackage should live in proto/subpackage.".into(),
                            }));
                        }
                    }

                    None
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();

        violations
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    static EXPECTED_HELP: &str = "Package names should be mirrored in folder structure, eg mypackage.subpackage should live in proto/subpackage.";

    #[test]
    fn file_name_should_not_matter() {
        let package = Package {
            name: "root".into(),
            files: vec!["proto/file_not_called_root.proto".into()],
            entities: Default::default(),
        };

        let result = PackageHierarchy.check_package(&package);
        assert!(result.is_empty());
    }

    #[test]
    fn should_enforce_hierarchy_on_1st_level() {
        let package = Package {
            name: "mypackage.subpackage".into(),
            files: vec!["proto/file.proto".into()],
            entities: Default::default(),
        };
        assert_eq!(
            PackageHierarchy.check_package(&package),
            &[Violation {
                rule: "PackageHierarchy".into(),
                level: Level::Error,
                message: violation::Message {
                    message: "expected file proto/file.proto to live in proto/subpackage.".into(),
                    help: EXPECTED_HELP.into(),
                },
                location: Default::default(),
                info: PackageHierarchy.rule_info().into(),
            }],
        );
    }

    #[test]
    fn should_complain_about_folder_name_mismatch() {
        let package = Package {
            name: "mypackage.subpackage".into(),
            files: vec!["proto/not_subpackage/file.proto".into()],
            entities: Default::default(),
        };
        assert_eq!(
            PackageHierarchy.check_package(&package),
            &[Violation {
                rule: "PackageHierarchy".into(),
                level: Level::Error,
                message: violation::Message {
                    message:
                        "expected file proto/not_subpackage/file.proto to live in proto/subpackage."
                            .into(),
                    help: EXPECTED_HELP.into(),
                },
                location: Default::default(),
                info: PackageHierarchy.rule_info().into(),
            }],
        );
    }

    #[test]
    fn should_check_each_file_of_a_package() {
        let package = Package {
            name: "mypackage.subpackage".into(),
            files: vec![
                "proto/subpackage/ok.proto".into(),
                "proto/not_subpackage/file.proto".into(),
                "proto/foo/bar/file.proto".into(),
            ],
            entities: Default::default(),
        };
        assert_eq!(
            PackageHierarchy.check_package(&package),
            &[
                Violation {
                rule: "PackageHierarchy".into(),
                level: Level::Error,
                message: violation::Message {
                    message:
                        "expected file proto/not_subpackage/file.proto to live in proto/subpackage."
                            .into(),
                    help: EXPECTED_HELP.into(),
                },
                location: Default::default(),
                info: PackageHierarchy.rule_info().into(),
                },
                Violation {
                    rule: "PackageHierarchy".into(),
                    level: Level::Error,
                    message: violation::Message {
                        message: "expected file proto/foo/bar/file.proto to live in proto/subpackage."
                            .into(),
                        help: EXPECTED_HELP.into(),
                    },
                    location: Default::default(),
                    info: PackageHierarchy.rule_info().into(),
                }
            ],
        );
    }
}
