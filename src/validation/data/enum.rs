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
