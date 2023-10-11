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

/// Error in difference between packages.
#[derive(Error, Debug)]
#[allow(missing_docs)]
pub enum PackagesDiffError {
    #[error("package {name} removed")]
    Removed { name: String },

    #[error("error in altered package {name}")]
    Package {
        name: String,
        #[source]
        error: PackageDiffError,
    },
}

impl PackagesDiff {
    /// Check packages diff for errors
    pub fn check(&self) -> Vec<PackagesDiffError> {
        let mut errors = vec![];

        for name in self.packages.removed.iter() {
            errors.push(PackagesDiffError::Removed { name: name.into() });
        }

        for (name, package) in self.packages.altered.iter() {
            for error in package.check().into_iter() {
                errors.push(PackagesDiffError::Package {
                    name: name.into(),
                    error,
                });
            }
        }

        errors
    }
}
