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

//! Buffrs Registry Context
//!
//! This type holds the necessary context for a buffrs registry. It is used to implement the
//! various APIs that this registry provides.

use crate::storage::AnyStorage;

/// Context
///
/// This contains all context needed for a buffrs registry.
#[derive(Clone, Debug)]
pub struct Context {
    storage: AnyStorage,
}

impl Context {
    /// Create a new context from a metadata instance and a storage instance.
    pub fn new(storage: AnyStorage) -> Self {
        Self { storage }
    }

    /// Get reference to the storage instance.
    pub fn storage(&self) -> &AnyStorage {
        &self.storage
    }
}
