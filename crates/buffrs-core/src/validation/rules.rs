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

use std::fmt::Debug;

use crate::{
    manifest::PackageManifest,
    validation::{
        data::*,
        violation::{self, *},
    },
};

mod ident_casing;
mod package_name;

pub use self::{ident_casing::*, package_name::*};

/// Collection of rules.
pub type RuleSet = Vec<Box<dyn Rule>>;

/// Rule to enforce for buffrs packages.
pub trait Rule: Debug {
    /// Name of this rule.
    ///
    /// Defaults to the name of the type of this rule.
    fn rule_name(&self) -> &'static str {
        std::any::type_name::<Self>().split("::").last().unwrap()
    }

    /// Help text for rule.
    fn rule_info(&self) -> &'static str;

    /// Default severity [`Level`] of the rule.
    fn rule_level(&self) -> Level {
        Level::Error
    }

    /// Turn a message into a violation.
    fn to_violation(&self, message: violation::Message) -> Violation {
        Violation {
            rule: self.rule_name().into(),
            level: self.rule_level(),
            message,
            location: Default::default(),
            info: self.rule_info().into(),
        }
    }

    /// Check [`Packages`] for violations.
    fn check_packages(&mut self, _packages: &Packages) -> Violations {
        vec![]
    }

    /// Check [`Package`] for violations.
    fn check_package(&mut self, _package: &Package) -> Violations {
        vec![]
    }

    /// Check [`Entity`] for violations.
    fn check_entity(&mut self, _name: &str, _entity: &Entity) -> Violations {
        vec![]
    }
}

impl Rule for RuleSet {
    fn rule_name(&self) -> &'static str {
        "RuleSet"
    }

    fn rule_info(&self) -> &'static str {
        "RuleSet"
    }

    fn check_packages(&mut self, packages: &Packages) -> Violations {
        self.iter_mut()
            .flat_map(|rule| rule.check_packages(packages))
            .collect()
    }

    fn check_package(&mut self, package: &Package) -> Violations {
        self.iter_mut()
            .flat_map(|rule| rule.check_package(package))
            .collect()
    }

    fn check_entity(&mut self, name: &str, entity: &Entity) -> Violations {
        self.iter_mut()
            .flat_map(|rule| rule.check_entity(name, entity))
            .collect()
    }
}

/// Get default rules for a given `buffrs` package name.
pub fn all(manifest: &PackageManifest) -> RuleSet {
    vec![
        Box::new(PackageName::new(manifest.name.clone())),
        Box::new(IdentCasing),
    ]
}
