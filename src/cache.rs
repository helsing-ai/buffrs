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

use std::path::PathBuf;

use crate::lock::FileRequirement;

/// If we are building a buffrs input directory, the file specified by this file requirement
/// must be stored here. This will return a relative path, which can be made relative to some root.
pub struct Entry(PathBuf);

impl From<FileRequirement> for Entry {
    fn from(req: FileRequirement) -> Entry {
        Self(
            format!(
                "{}-{}.tgz",
                req.digest.algorithm(),
                hex::encode(req.digest.as_bytes())
            )
            .into(),
        )
    }
}
