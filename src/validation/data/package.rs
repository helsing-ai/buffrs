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

/// Protocol buffer package.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Diff)]
#[diff(attr(
    #[derive(Debug)]
    #[allow(missing_docs)]
))]
pub struct Package {
    /// Name of the package.
    pub name: String,
    /// File path where this package is defined.
    pub file: PathBuf,
    /// Entities defined in this package.
    pub entities: BTreeMap<String, Entity>,
}

/// Error parsing package.
#[derive(Error, Debug, Diagnostic)]
#[allow(missing_docs)]
pub enum PackageError {
    #[error("duplicate entity {entity} in file {file}")]
    #[diagnostic(
        help = "check to make sure your don't define two entities of the same name",
        code = "duplicate_entity"
    )]
    DuplicateEntity { file: PathBuf, entity: String },

    #[error("error parsing message {name}")]
    Message {
        name: String,
        #[source]
        #[diagnostic_source]
        error: MessageError,
    },

    #[error("error parsing enum {name}")]
    Enum {
        name: String,
        #[source]
        #[diagnostic_source]
        error: EnumError,
    },
}

impl Package {
    /// Try to create a new one from a [`FileDescriptorProto`].
    pub fn new(descriptor: &FileDescriptorProto) -> Result<Self, PackageError> {
        let mut package = Self {
            file: descriptor.name().into(),
            name: descriptor.package().to_string(),
            entities: Default::default(),
        };

        for message in &descriptor.message_type {
            package.add_entity(
                message.name(),
                Message::new(message).map_err(|error| PackageError::Message {
                    name: message.name().into(),
                    error,
                })?,
            )?;
        }

        for entity in &descriptor.enum_type {
            package.add_entity(
                entity.name(),
                Enum::new(entity).map_err(|error| PackageError::Enum {
                    name: entity.name().into(),
                    error,
                })?,
            )?;
        }

        for entity in &descriptor.service {
            package.add_entity(entity.name(), Service {})?;
        }

        Ok(package)
    }

    /// Try to add an entity.
    fn add_entity<T: Into<Entity>>(&mut self, name: &str, entity: T) -> Result<(), PackageError> {
        match self.entities.entry(name.into()) {
            Entry::Vacant(entry) => {
                entry.insert(entity.into());
                Ok(())
            }
            Entry::Occupied(_entry) => Err(PackageError::DuplicateEntity {
                file: self.file.clone(),
                entity: name.into(),
            }),
        }
    }

    /// Check this [`Package`] against a [`RuleSet`] for violations.
    pub fn check(&self, rules: &mut RuleSet) -> Violations {
        rules
            .check_package(self)
            .into_iter()
            .chain(
                self.entities
                    .iter()
                    .flat_map(|(name, entity)| rules.check_entity(name, entity).into_iter()),
            )
            .map(|mut violation| {
                violation.location.file = Some(self.file.display().to_string());
                violation.location.package = Some(self.name.clone());
                violation
            })
            .collect()
    }
}
