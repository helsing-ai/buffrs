use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Workspace implementation that follows Cargo conventions
///
/// https://doc.rust-lang.org/cargo/reference/workspaces.html#the-members-and-exclude-fields
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Workspace {
    /// Packages to include in the workspace.
    pub members: Option<Vec<String>>,
    /// Packages to exclude from the workspace.
    pub exclude: Option<Vec<String>>,
}

impl Workspace {
    /// Resolves workspace members
    ///
    /// Constraints:
    /// 1. Only goes 1 level deep - patterns like "packages/*" or "lib" are supported
    /// 2. Only includes subdirectories that have a `Proto.toml` in their root
    /// 3. Exclude supports glob patterns for filtering
    ///
    /// # Example
    ///
    /// ```toml
    /// [workspace]
    /// members = ["packages/*", "special"]
    /// exclude = ["packages/internal*"]
    /// ```
    pub fn resolve_members(
        &self,
        workspace_root: impl AsRef<Path>,
    ) -> miette::Result<Vec<PathBuf>> {
        use crate::manifest::MANIFEST_FILE;
        use miette::{IntoDiagnostic, WrapErr};
        use std::collections::BTreeSet;
        use std::fs;

        // Default to ["*"] if members is not specified
        let default_members = vec!["*".to_string()];
        let member_patterns = self.members.as_ref().unwrap_or(&default_members);
        let exclude_patterns = self.exclude.as_ref().map(|e| e.as_slice()).unwrap_or(&[]);

        let mut resolved_members = BTreeSet::new();

        // Process each member pattern
        for pattern in member_patterns {
            if pattern.contains('*') || pattern.contains('?') || pattern.contains('[') {
                // Glob pattern - only 1 level deep
                let pattern_matcher = glob::Pattern::new(pattern)
                    .into_diagnostic()
                    .wrap_err_with(|| miette::miette!("invalid glob pattern: {}", pattern))?;

                // Read all entries in workspace root
                let entries = fs::read_dir(workspace_root.as_ref())
                    .into_diagnostic()
                    .wrap_err_with(|| miette::miette!("failed to read workspace directory"))?;

                for entry in entries {
                    let entry = entry
                        .into_diagnostic()
                        .wrap_err_with(|| miette::miette!("failed to read directory entry"))?;

                    let path = entry.path();
                    if path.is_dir() {
                        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                            if pattern_matcher.matches(name) && path.join(MANIFEST_FILE).exists() {
                                resolved_members.insert(PathBuf::from(name));
                            }
                        }
                    }
                }
            } else {
                // Literal path - check if it exists and has Proto.toml
                let member_path = workspace_root.as_ref().join(pattern);
                if member_path.is_dir() && member_path.join(MANIFEST_FILE).exists() {
                    resolved_members.insert(PathBuf::from(pattern));
                }
            }
        }

        // Filter out excluded patterns
        let final_members: Vec<PathBuf> = resolved_members
            .into_iter()
            .filter(|member| {
                let member_str = member.to_str().unwrap_or("");

                // Check if this member matches any exclude pattern
                !exclude_patterns.iter().any(|exclude_pattern| {
                    if let Ok(glob_matcher) = glob::Pattern::new(exclude_pattern) {
                        glob_matcher.matches(member_str)
                    } else {
                        member_str == exclude_pattern
                    }
                })
            })
            .collect();

        Ok(final_members)
    }
}
