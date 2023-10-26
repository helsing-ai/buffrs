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

/// Entity that can be defined in a protocol buffer file.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Diff)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[diff(attr(
    #[derive(Debug)]
    #[allow(missing_docs)]
))]
pub enum Entity {
    /// Enumeration.
    Enum(Enum),
    /// Service definition.
    Service(Service),
    /// Message definition.
    Message(Message),
}

impl From<Enum> for Entity {
    fn from(entity: Enum) -> Self {
        Self::Enum(entity)
    }
}

impl From<Service> for Entity {
    fn from(entity: Service) -> Self {
        Self::Service(entity)
    }
}

impl From<Message> for Entity {
    fn from(entity: Message) -> Self {
        Self::Message(entity)
    }
}

impl Entity {
    /// Check [`Entity`] against [`RuleSet`] for [`Violations`].
    pub fn check(&self, _rules: &mut RuleSet) -> Violations {
        Violations::default()
    }
}
