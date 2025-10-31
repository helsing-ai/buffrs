// (c) Copyright 2025 Helsing GmbH. All rights reserved.

use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

use glob::Pattern;
use miette::{IntoDiagnostic, Result, WrapErr, miette};
use serde::{Deserialize, Serialize};

use crate::manifest::MANIFEST_FILE;

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
    pub fn resolve_members(&self, workspace_root: impl AsRef<Path>) -> Result<Vec<PathBuf>> {
        // Default to ["*"] if members is not specified

        let member_patterns = &self.members;
        let exclude_patterns = self.exclude.as_deref().unwrap_or(&[]);

        let mut resolved_members = BTreeSet::new();

        // Process each member pattern
        for pattern in member_patterns {
            if pattern.contains(['*', '?', '[']) {
                // Glob pattern - only 1 level deep
                let pattern_matcher = Pattern::new(pattern)
                    .into_diagnostic()
                    .wrap_err_with(|| miette!("invalid glob pattern: {}", pattern))?;

                // Read all entries in workspace root
                let entries = fs::read_dir(workspace_root.as_ref())
                    .into_diagnostic()
                    .wrap_err_with(|| miette!("failed to read workspace directory"))?;

                for entry in entries {
                    let entry = entry
                        .into_diagnostic()
                        .wrap_err_with(|| miette!("failed to read directory entry"))?;

                    let path = entry.path();
                    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                        if path.is_dir()
                            && pattern_matcher.matches(name)
                            && path.join(MANIFEST_FILE).exists()
                        {
                            resolved_members.insert(PathBuf::from(name));
                        }
                    }
                }
            } else {
                // Literal path - check if it exists and has Proto.toml
                let member_path = workspace_root.as_ref().join(pattern);
                if member_path.is_dir() && member_path.join(MANIFEST_FILE).exists() {
                    resolved_members.insert(PathBuf::from(pattern));
                } else {
                    tracing::warn!(
                        ":: path {} was explicitly provided in members, but contains no buffrs manifest",
                        member_path.display()
                    );
                }
            }
        }

        // Filter out excluded patterns
        let final_members: Vec<PathBuf> = resolved_members
            .into_iter()
            .map(|member| {
                let member_str = member.to_str().ok_or_else(|| {
                    miette!(
                        "workspace member path is not valid UTF-8: {}",
                        member.display()
                    )
                })?;

                let is_excluded = exclude_patterns.iter().any(|exclude_pattern| {
                    if let Ok(glob_matcher) = Pattern::new(exclude_pattern) {
                        glob_matcher.matches(member_str)
                    } else {
                        member_str == exclude_pattern
                    }
                });

                Ok((!is_excluded).then_some(member))
            })
            // collecting Result<T, E> into Result<Vec<T>, E> short-circuit into Err on first Err in Vec
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .flatten()
            .collect();

        Ok(final_members)
    }
}

#[cfg(test)]
mod tests {
    mod workspace_tests {
        use super::super::*;
        use std::path::PathBuf;

        #[test]
        fn test_resolve_members_with_explicit_members() {
            use std::fs;

            let temp_dir = tempfile::tempdir().unwrap();
            let workspace_root = temp_dir.path();

            // Create member directories with Proto.toml files
            let pkg1 = workspace_root.join("package1");
            let pkg2 = workspace_root.join("package2");
            fs::create_dir(&pkg1).unwrap();
            fs::create_dir(&pkg2).unwrap();
            fs::write(pkg1.join(MANIFEST_FILE), "").unwrap();
            fs::write(pkg2.join(MANIFEST_FILE), "").unwrap();

            let workspace = Workspace {
                members: vec!["package1".to_string(), "package2".to_string()],
                exclude: None,
            };

            let members = workspace.resolve_members(workspace_root).unwrap();
            assert_eq!(members.len(), 2);
            assert!(members.contains(&PathBuf::from("package1")));
            assert!(members.contains(&PathBuf::from("package2")));
        }

        #[test]
        fn test_resolve_members_with_glob_pattern() {
            use std::fs;

            let temp_dir = tempfile::tempdir().unwrap();
            let workspace_root = temp_dir.path();

            // Create multiple packages at workspace root (1 level deep only)
            let pkg1 = workspace_root.join("pkg1");
            let pkg2 = workspace_root.join("pkg2");
            let lib = workspace_root.join("lib-special");
            fs::create_dir(&pkg1).unwrap();
            fs::create_dir(&pkg2).unwrap();
            fs::create_dir(&lib).unwrap();
            fs::write(pkg1.join(MANIFEST_FILE), "").unwrap();
            fs::write(pkg2.join(MANIFEST_FILE), "").unwrap();
            fs::write(lib.join(MANIFEST_FILE), "").unwrap();

            let workspace = Workspace {
                members: vec!["pkg*".to_string()],
                exclude: None,
            };

            let members = workspace.resolve_members(workspace_root).unwrap();
            assert_eq!(members.len(), 2);
            assert!(members.contains(&PathBuf::from("pkg1")));
            assert!(members.contains(&PathBuf::from("pkg2")));
            assert!(!members.contains(&PathBuf::from("lib-special")));
        }

        #[test]
        fn test_resolve_members_with_exclude() {
            use std::fs;

            let temp_dir = tempfile::tempdir().unwrap();
            let workspace_root = temp_dir.path();

            // Create packages at workspace root
            let pkg1 = workspace_root.join("pkg1");
            let pkg2 = workspace_root.join("pkg2");
            let internal = workspace_root.join("internal");

            fs::create_dir(&pkg1).unwrap();
            fs::create_dir(&pkg2).unwrap();
            fs::create_dir(&internal).unwrap();

            fs::write(pkg1.join(MANIFEST_FILE), "").unwrap();
            fs::write(pkg2.join(MANIFEST_FILE), "").unwrap();
            fs::write(internal.join(MANIFEST_FILE), "").unwrap();

            let workspace = Workspace {
                members: vec!["*".to_string()],
                exclude: Some(vec!["internal".to_string()]),
            };

            let members = workspace.resolve_members(workspace_root).unwrap();
            assert_eq!(members.len(), 2);
            assert!(members.contains(&PathBuf::from("pkg1")));
            assert!(members.contains(&PathBuf::from("pkg2")));
            assert!(!members.contains(&PathBuf::from("internal")));
        }

        #[test]
        fn test_resolve_members_with_exclude_glob() {
            use std::fs;

            let temp_dir = tempfile::tempdir().unwrap();
            let workspace_root = temp_dir.path();

            // Create packages at workspace root
            let pkg1 = workspace_root.join("pkg1");
            let pkg2 = workspace_root.join("pkg2");
            let internal1 = workspace_root.join("internal-one");
            let internal2 = workspace_root.join("internal-two");

            fs::create_dir(&pkg1).unwrap();
            fs::create_dir(&pkg2).unwrap();
            fs::create_dir(&internal1).unwrap();
            fs::create_dir(&internal2).unwrap();

            fs::write(pkg1.join(MANIFEST_FILE), "").unwrap();
            fs::write(pkg2.join(MANIFEST_FILE), "").unwrap();
            fs::write(internal1.join(MANIFEST_FILE), "").unwrap();
            fs::write(internal2.join(MANIFEST_FILE), "").unwrap();

            let workspace = Workspace {
                members: vec!["*".to_string()],
                exclude: Some(vec!["internal*".to_string()]),
            };

            let members = workspace.resolve_members(workspace_root).unwrap();
            assert_eq!(members.len(), 2);
            assert!(members.contains(&PathBuf::from("pkg1")));
            assert!(members.contains(&PathBuf::from("pkg2")));
            assert!(!members.contains(&PathBuf::from("internal-one")));
            assert!(!members.contains(&PathBuf::from("internal-two")));
        }

        #[test]
        fn test_resolve_members_ignores_dirs_without_manifest() {
            use std::fs;

            let temp_dir = tempfile::tempdir().unwrap();
            let workspace_root = temp_dir.path();

            // Create directory with Proto.toml
            let pkg1 = workspace_root.join("pkg1");
            fs::create_dir(&pkg1).unwrap();
            fs::write(pkg1.join(MANIFEST_FILE), "").unwrap();

            // Create directory WITHOUT Proto.toml
            let not_a_pkg = workspace_root.join("not-a-package");
            fs::create_dir(&not_a_pkg).unwrap();

            let workspace = Workspace {
                members: vec!["*".to_string()],
                exclude: None,
            };

            let members = workspace.resolve_members(workspace_root).unwrap();
            assert_eq!(members.len(), 1);
            assert!(members.contains(&PathBuf::from("pkg1")));
            assert!(!members.contains(&PathBuf::from("not-a-package")));
        }

        #[test]
        fn test_resolve_members_mixed_patterns() {
            use std::fs;

            let temp_dir = tempfile::tempdir().unwrap();
            let workspace_root = temp_dir.path();

            // Create packages at workspace root
            let pkg1 = workspace_root.join("pkg1");
            let pkg2 = workspace_root.join("pkg2");
            let special = workspace_root.join("special");

            fs::create_dir(&pkg1).unwrap();
            fs::create_dir(&pkg2).unwrap();
            fs::create_dir(&special).unwrap();
            fs::write(pkg1.join(MANIFEST_FILE), "").unwrap();
            fs::write(pkg2.join(MANIFEST_FILE), "").unwrap();
            fs::write(special.join(MANIFEST_FILE), "").unwrap();

            let workspace = Workspace {
                members: vec!["pkg*".to_string(), "special".to_string()],
                exclude: None,
            };

            let members = workspace.resolve_members(workspace_root).unwrap();
            assert_eq!(members.len(), 3);
            assert!(members.contains(&PathBuf::from("pkg1")));
            assert!(members.contains(&PathBuf::from("pkg2")));
            assert!(members.contains(&PathBuf::from("special")));
        }

        #[test]
        fn test_resolve_members_deterministic_ordering() {
            use std::fs;

            let temp_dir = tempfile::tempdir().unwrap();
            let workspace_root = temp_dir.path();

            // Create members in non-alphabetical order
            for name in ["zebra", "alpha", "beta"] {
                let dir = workspace_root.join(name);
                fs::create_dir(&dir).unwrap();
                fs::write(dir.join(MANIFEST_FILE), "").unwrap();
            }

            let workspace = Workspace {
                members: vec!["*".to_string()],
                exclude: None,
            };

            let members = workspace.resolve_members(workspace_root).unwrap();

            // Should be sorted alphabetically
            assert_eq!(members.len(), 3);
            assert_eq!(members[0], PathBuf::from("alpha"));
            assert_eq!(members[1], PathBuf::from("beta"));
            assert_eq!(members[2], PathBuf::from("zebra"));
        }
    }
}
