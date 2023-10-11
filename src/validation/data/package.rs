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
        let mut violations = rules.check_package(self);
        for (name, entity) in self.entities.iter() {
            violations.append(&mut rules.check_entity(name, entity));
            violations.append(&mut entity.check(rules));
        }
        for violation in &mut violations {
            violation.location.file = Some(self.file.display().to_string());
            violation.location.package = Some(self.name.clone());
        }
        violations
    }
}
