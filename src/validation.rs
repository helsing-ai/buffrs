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
/// Rules for protocol buffer definitions.
mod rules;
/// Serde utilities.
pub(crate) mod serde;

mod parse;
#[cfg(test)]
mod tests;
mod violation;

use self::parse::*;

pub use self::violation::*;

use miette::IntoDiagnostic;
use std::path::Path;

pub struct Validator {
    parser: parse::Parser,
    package: String,
}

impl Validator {
    /// Create new parser with a given root path.
    pub fn new(root: &Path, package: &str) -> Self {
        Self {
            parser: parse::Parser::new(root),
            package: package.into(),
        }
    }

    pub fn input(&mut self, file: &Path) {
        self.parser.input(file);
    }

    pub fn validate(self) -> miette::Result<Violations> {
        let parsed = self.parser.parse().into_diagnostic()?;
        let mut rule_set = rules::package_rules(&self.package);
        Ok(parsed.check(&mut rule_set))
    }
}
