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
use std::path::PathBuf;

/// Error in difference between package
#[derive(Error, Debug)]
#[allow(missing_docs)]
pub enum PackageDiffError {
    #[error("file path changed to {path:?}")]
    PathChanged { path: PathBuf },

    #[error("entity {name} removed")]
    EntityRemoved { name: String },

    #[error("error in entity {name}")]
    Entity {
        name: String,
        #[source]
        error: EntityDiffError,
    },
}

impl PackageDiff {
    /// Check package diff for errors
    pub fn check(&self) -> Vec<PackageDiffError> {
        let mut errors = vec![];

        if let Some(path) = &self.name {
            errors.push(PackageDiffError::PathChanged { path: path.into() });
        }

        for name in self.entities.removed.iter() {
            errors.push(PackageDiffError::EntityRemoved { name: name.into() });
        }

        for (name, entity) in self.entities.altered.iter() {
            for error in entity.check().into_iter() {
                errors.push(PackageDiffError::Entity {
                    name: name.into(),
                    error,
                });
            }
        }

        errors
    }
}
