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

/// Error parsing package.
#[derive(Error, Debug, Diagnostic)]
#[allow(missing_docs)]
pub enum EnumError {
    #[error("missing value number")]
    ValueNumberMissing,

    #[error("missing value name")]
    ValueNameMissing,
}

/// Enumeration definition.
#[derive(Serialize, Deserialize, Default, Clone, Debug, PartialEq, Eq, Diff)]
#[diff(attr(
    #[derive(Debug)]
    #[allow(missing_docs)]
))]
pub struct Enum {
    /// Variants of this enum.
    #[serde(deserialize_with = "crate::validation::serde::de_int_key")]
    pub values: BTreeMap<i32, EnumValue>,
}

impl Enum {
    /// Attempt to create new from [`EnumDescriptorProto`].
    pub fn new(descriptor: &EnumDescriptorProto) -> Result<Self, EnumError> {
        let mut entity = Self::default();

        for value in &descriptor.value {
            entity.add(value)?;
        }

        Ok(entity)
    }

    /// Add an [`EnumValue`] to this enum definition.
    pub fn add(&mut self, value: &EnumValueDescriptorProto) -> Result<(), EnumError> {
        let number = value.number.ok_or(EnumError::ValueNumberMissing)?;
        self.values.insert(number, EnumValue::new(value)?);
        Ok(())
    }
}

/// Single value for an [`Enum`].
#[derive(Serialize, Deserialize, Default, Clone, Debug, PartialEq, Eq, Diff)]
#[diff(attr(
    #[derive(Debug)]
    #[allow(missing_docs)]
))]
pub struct EnumValue {
    /// Name of this enum value.
    pub name: String,
}

impl EnumValue {
    /// Attempt to create new from [`EnumValueDescriptorProto`].
    pub fn new(descriptor: &EnumValueDescriptorProto) -> Result<Self, EnumError> {
        Ok(Self {
            name: descriptor.name.clone().ok_or(EnumError::ValueNameMissing)?,
        })
    }
}
