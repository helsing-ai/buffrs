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

use std::{
    collections::{btree_map::Entry, BTreeMap},
    path::PathBuf,
};

use diff::Diff;
use miette::Diagnostic;
use protobuf::descriptor::{
    field_descriptor_proto::{Label as FieldDescriptorLabel, Type as FieldDescriptorType},
    *,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::validation::{
    rules::{Rule, RuleSet},
    Violations,
};

mod entity;
mod r#enum;
mod message;
mod package;
mod packages;
mod service;

pub use self::{entity::*, message::*, package::*, packages::*, r#enum::*, service::*};
