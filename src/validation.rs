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

/// Parsed protocol buffer definitions.
mod data;
mod parse;
/// Rules for protocol buffer definitions.
mod rules;
/// Serde utilities.
pub(crate) mod serde;
mod violation;

pub use violation::*;

use miette::IntoDiagnostic;
use std::path::Path;

use self::parse::*;
use crate::package::PackageName;

/// Validates buffrs packages.
///
/// This allows running validations on top of buffrs packages.
pub struct Validator {
    parser: parse::Parser,
    package: String,
}

impl Validator {
    /// Create new parser with a given root path.
    pub fn new(root: &Path, package: &PackageName) -> Self {
        Self {
            parser: Parser::new(root),
            package: package.to_string(),
        }
    }

    /// Add file to be validated.
    pub fn input(&mut self, file: &Path) {
        self.parser.input(file);
    }

    /// Run validation.
    ///
    /// This produces a list of [`Violation`]. These implement the
    /// [`Diagnostic`](miette::Diagnostic) trait which gives them important metadata, such as the
    /// severity.
    pub fn validate(self) -> miette::Result<Violations> {
        let parsed = self.parser.parse().into_diagnostic()?;
        let mut rule_set = rules::package_rules(&self.package);
        Ok(parsed.check(&mut rule_set))
    }
}

#[cfg(test)]
mod tests {
    use paste::paste;

    macro_rules! parse_test {
        ($name:ident) => {
            paste! {
                #[test]
                fn [< can_parse_ $name >]() {
                    use std::path::Path;
                    let mut parser = super::Parser::new(Path::new("tests/data/parsing"));
                    parser.input(std::path::Path::new(concat!("tests/data/parsing/", stringify!($name), ".proto")));
                    let packages = parser.parse().unwrap();
                    let expected = include_str!(concat!("../tests/data/parsing/", stringify!($name), ".json"));
                    let expected = serde_json::from_str(&expected).unwrap();
                    similar_asserts::assert_eq!(packages, expected);
                }
            }
        };
    }

    parse_test!(books);
    parse_test!(addressbook);
}
