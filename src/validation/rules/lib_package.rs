use super::*;

#[derive(Debug, Clone, Copy)]
pub struct LibPackage;

impl Rule for LibPackage {
    fn rule_info(&self) -> &'static str {
        "Make sure that lib packages don't contain service definitions."
    }

    fn rule_level(&self) -> Level {
        Level::Error
    }

    fn check_package(&mut self, package: &Package) -> Violations {
        let package_name = &package.name;
        package
            .entities
            .iter()
            .filter_map(|(name, entity)| {
                if let Entity::Service(_) = entity {
                    let message = violation::Message {
                        message: format!("{name} is a service definition but {package_name} is a lib type package and thus shouldn't contain services."),
                        help: "It's best to keep packages containing services separate from packages containing messages and enums.".into(),
                    };
                    Some(self.to_violation(message))
                } else {
                    None
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    #[test]
    fn should_complain_about_service_defs() {
        let entities = {
            let mut ret: BTreeMap<String, Entity> = BTreeMap::new();
            ret.insert("service A".into(), Entity::Service(Service {}));
            ret.insert("service B".into(), Entity::Service(Service {}));

            ret
        };

        let package = Package {
            name: "my package".into(),
            files: vec!["ignored.proto".into()],
            entities,
        };

        let violations = LibPackage.check_package(&package);

        let help = "It's best to keep packages containing services separate from packages containing messages and enums.";
        let violation_1 = Violation {
            rule: "LibPackage".into(),
            level: Level::Error,
            message: violation::Message {
                message: "service A is a service definition but my package is a lib type package and thus shouldn't contain services.".into(),
                help: help.into(),
            },
            location: Default::default(),
            info: LibPackage.rule_info().into(),
        };
        let violation_2 = Violation {
            message: violation::Message {
                message: "service B is a service definition but my package is a lib type package and thus shouldn't contain services.".into(),
                help: help.into(),
            },
            ..violation_1.clone()
        };

        assert_eq!(violations, vec![violation_1, violation_2]);
    }
}
