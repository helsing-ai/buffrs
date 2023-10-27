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
pub mod data;
/// Rules for protocol buffer definitions.
pub mod rules;
/// Serde utilities.
pub(crate) mod serde;

mod parse;
#[cfg(test)]
mod tests;
mod violation;

pub use self::{
    parse::{ParseError, Parser},
    violation::*,
};
