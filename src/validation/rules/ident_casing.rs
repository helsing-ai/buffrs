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

/// Ensure that file names match package names.
#[derive(Debug, Clone, Default)]
pub struct IdentCasing;

impl Rule for IdentCasing {
    fn rule_info(&self) -> &'static str {
        "making sure entity names are correct"
    }

    /// Default severity [`Level`] of the rule.
    fn rule_level(&self) -> Level {
        Level::Info
    }

    /// Check [`Entity`] for violations.
    fn check_entity(&mut self, _name: &str, _entity: &Entity) -> Violations {
        Default::default()
    }
}
