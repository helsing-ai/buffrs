// Copyright 2026 Helsing GmbH
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

use std::{path::Path, str::FromStr};

use async_trait::async_trait;
use miette::{Context, IntoDiagnostic};
use serde::{Deserialize, Serialize};

use super::MANIFEST_FILE;
use super::raw::RawManifest;
use crate::{ManagedFile, errors::DeserializationError, io::File};

/// A manifest for a buffrs workspace
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceManifest {
    /// Definition of a buffrs workspace
    pub workspace: Workspace,
}

impl WorkspaceManifest {
    /// Create a new builder for WorkspaceManifest
    pub fn builder() -> WorkspaceManifestBuilder<NoWorkspace> {
        WorkspaceManifestBuilder {
            workspace: NoWorkspace,
        }
    }
}

/// NoWorkspace Type used for the workspace typestate builder pattern
#[derive(Default, Clone)]
pub struct NoWorkspace;

/// Builder for constructing a WorkspaceManifest
pub struct WorkspaceManifestBuilder<W> {
    workspace: W,
}

impl WorkspaceManifestBuilder<NoWorkspace> {
    /// Set the workspace and transition to a WorkspaceManifestBuilder<Workspace>
    pub fn workspace(self, workspace: Workspace) -> WorkspaceManifestBuilder<Workspace> {
        WorkspaceManifestBuilder { workspace }
    }
}

impl WorkspaceManifestBuilder<Workspace> {
    /// Builds the WorkspaceManifest
    pub fn build(self) -> WorkspaceManifest {
        WorkspaceManifest {
            workspace: self.workspace,
        }
    }
}

#[async_trait]
impl File for WorkspaceManifest {
    const DEFAULT_PATH: &str = MANIFEST_FILE;

    async fn load_from<P>(path: P) -> miette::Result<Self>
    where
        P: AsRef<Path> + Send + Sync,
    {
        RawManifest::load_from(path).await?.try_into()
    }

    async fn save<P>(&self, path: P) -> miette::Result<()>
    where
        P: AsRef<Path> + Send + Sync,
    {
        RawManifest::from(self.clone()).save(path).await
    }
}

impl FromStr for WorkspaceManifest {
    type Err = miette::Report;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        input
            .parse::<RawManifest>()
            .into_diagnostic()
            .wrap_err(DeserializationError(ManagedFile::Manifest))
            .map(WorkspaceManifest::try_from)?
    }
}

impl TryInto<String> for WorkspaceManifest {
    type Error = toml::ser::Error;

    fn try_into(self) -> Result<String, Self::Error> {
        toml::to_string_pretty(&RawManifest::from(self))
    }
}

/// Workspace implementation that follows Cargo conventions
///
/// https://doc.rust-lang.org/cargo/reference/workspaces.html#the-members-and-exclude-fields
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Workspace {
    /// Packages to include in the workspace.
    pub members: Vec<String>,
    /// Packages to exclude from the workspace.
    pub exclude: Option<Vec<String>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::manifest::RawManifest;
    use std::collections::HashMap;

    #[test]
    fn workspace_manifest_builder() {
        let workspace = Workspace {
            members: vec!["pkg1".to_string(), "pkg2".to_string()],
            exclude: Some(vec!["internal".to_string()]),
        };

        let manifest = WorkspaceManifest::builder()
            .workspace(workspace.clone())
            .build();

        assert_eq!(manifest.workspace, workspace);
    }

    #[test]
    fn workspace_manifest_from_str() {
        let toml = r#"
                [workspace]
                members = ["pkg1", "pkg2"]
            "#;

        let manifest = WorkspaceManifest::from_str(toml).expect("should parse");
        assert_eq!(manifest.workspace.members, vec!["pkg1", "pkg2"]);
    }

    #[test]
    fn workspace_manifest_from_str_with_exclude() {
        let toml = r#"
                [workspace]
                members = ["packages/*"]
                exclude = ["packages/internal"]
            "#;

        let manifest = WorkspaceManifest::from_str(toml).expect("should parse");
        assert_eq!(manifest.workspace.members, vec!["packages/*"]);
        assert_eq!(
            manifest.workspace.exclude,
            Some(vec!["packages/internal".to_string()])
        );
    }

    #[test]
    fn workspace_manifest_to_raw_manifest() {
        let workspace = Workspace {
            members: vec!["pkg1".to_string()],
            exclude: None,
        };

        let manifest = WorkspaceManifest::builder().workspace(workspace).build();
        let raw: RawManifest = manifest.into();

        assert!(matches!(raw, RawManifest::Canary { .. }));
        assert!(raw.workspace().is_some());
        assert_eq!(raw.package(), None);
        assert_eq!(raw.dependencies(), None);
    }

    #[test]
    fn workspace_manifest_roundtrip() {
        let toml = r#"
                [workspace]
                members = ["pkg1", "pkg2"]
            "#;

        let manifest = WorkspaceManifest::from_str(toml).expect("should parse");
        let serialized: String = manifest.try_into().expect("should serialize");

        assert!(serialized.contains("[workspace]"));
        assert!(serialized.contains("pkg1"));
        assert!(serialized.contains("pkg2"));
    }

    #[test]
    fn workspace_manifest_try_from_raw_missing_workspace_errors() {
        let raw = RawManifest::Canary {
            package: None,
            dependencies: Some(HashMap::new()),
            workspace: None,
        };

        let result = WorkspaceManifest::try_from(raw);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("no workspace manifest")
        );
    }
}
