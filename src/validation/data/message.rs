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

/// Error converting parsed protobuf fileset into custom representation.
#[derive(Error, Debug, Diagnostic)]
#[allow(missing_docs, clippy::enum_variant_names)]
pub enum MessageError {
    #[error("field number missing")]
    FieldNumberMissing,
    #[error("field name missing")]
    FieldNameMissing,
    #[error("field type missing")]
    FieldTypeMissing,
}

/// Message definition.
#[derive(Serialize, Deserialize, Default, Clone, Debug, PartialEq, Eq, Diff)]
#[diff(attr(
    #[derive(Debug)]
    #[allow(missing_docs)]
))]
pub struct Message {
    /// Fields defined in this message.
    #[serde(deserialize_with = "crate::validation::serde::de_int_key")]
    pub fields: BTreeMap<i32, Field>,
}

impl Message {
    /// Try to create new [`Message`] from [`DescriptorProto`].
    pub fn new(descriptor: &DescriptorProto) -> Result<Self, MessageError> {
        let mut message = Message::default();

        for field in &descriptor.field {
            message.fields.insert(
                field.number.ok_or(MessageError::FieldNumberMissing)?,
                Field::new(field)?,
            );
        }

        Ok(message)
    }
}

/// Field defined in this message.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Diff)]
#[diff(attr(
    #[derive(Debug)]
    #[allow(missing_docs)]
))]
pub struct Field {
    /// Name of field.
    pub name: String,
    /// Type of field.
    pub type_: FieldType,
    /// Label of field.
    pub label: Option<FieldLabel>,
    /// Default value.
    pub default: Option<String>,
}

impl Field {
    /// Try to create a new [`Field`] from a [`FieldDescriptorProto`].
    fn new(descriptor: &FieldDescriptorProto) -> Result<Self, MessageError> {
        Ok(Self {
            name: descriptor
                .name
                .clone()
                .ok_or(MessageError::FieldNameMissing)?,
            type_: match descriptor
                .type_
                .ok_or(MessageError::FieldTypeMissing)?
                .enum_value()
            {
                Ok(value) => value.into(),
                Err(number) => FieldType::Unknown(number),
            },
            label: match descriptor.label.map(|label| label.enum_value()) {
                None => None,
                Some(Ok(label)) => Some(label.into()),
                Some(Err(number)) => Some(FieldLabel::Unknown(number)),
            },
            default: descriptor.default_value.clone(),
        })
    }
}

/// Built-in field types.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Diff)]
#[serde(rename_all = "snake_case")]
#[diff(attr(
    #[derive(Debug)]
    #[allow(missing_docs)]
))]
#[allow(missing_docs)]
pub enum FieldType {
    Double,
    Float,
    Int64,
    Uint64,
    Int32,
    Fixed64,
    Fixed32,
    Bool,
    String,
    Group,
    Message,
    Bytes,
    Uint32,
    Enum,
    Sfixed32,
    Sfixed64,
    Sint32,
    Sint64,
    Unknown(i32),
}

impl From<FieldDescriptorType> for FieldType {
    fn from(type_: FieldDescriptorType) -> Self {
        use FieldDescriptorType::*;
        use FieldType::*;
        match type_ {
            TYPE_DOUBLE => Double,
            TYPE_FLOAT => Float,
            TYPE_INT64 => Int64,
            TYPE_UINT64 => Uint64,
            TYPE_INT32 => Int32,
            TYPE_FIXED64 => Fixed64,
            TYPE_FIXED32 => Fixed32,
            TYPE_BOOL => Bool,
            TYPE_STRING => String,
            TYPE_GROUP => Group,
            TYPE_MESSAGE => Message,
            TYPE_BYTES => Bytes,
            TYPE_UINT32 => Uint32,
            TYPE_ENUM => Enum,
            TYPE_SFIXED32 => Sfixed32,
            TYPE_SFIXED64 => Sfixed64,
            TYPE_SINT32 => Sint32,
            TYPE_SINT64 => Sint64,
        }
    }
}

/// Field label.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Diff)]
#[serde(rename_all = "snake_case")]
#[diff(attr(
    #[derive(Debug)]
    #[allow(missing_docs)]
))]
#[allow(missing_docs)]
pub enum FieldLabel {
    Optional,
    Required,
    Repeated,
    Unknown(i32),
}

impl From<FieldDescriptorLabel> for FieldLabel {
    fn from(label: FieldDescriptorLabel) -> Self {
        use FieldDescriptorLabel::*;
        use FieldLabel::*;
        match label {
            LABEL_OPTIONAL => Optional,
            LABEL_REQUIRED => Required,
            LABEL_REPEATED => Repeated,
        }
    }
}
