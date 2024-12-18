use std::path::{Component, Path, PathBuf};

/// Extension trait for `Path`.
pub trait PathUtil {
    /// Convert a path to a string, replacing backslashes with forward slashes on Windows.
    fn to_posix_string(&self) -> String;

    /// Get the relative path of `self` from `base`.
    ///
    /// If `self` is not a subpath of `base`, the relative path
    /// uses `..` to traverse up to `base`. If `this` isn't reachable
    /// from `base`, `None` is returned.
    ///
    /// # Warning
    ///
    /// - This function does not check if the paths are valid or if they exist.
    /// - Hence, this function does not consider symbolic links.
    ///
    /// # Arguments
    /// * `base` - The base path
    fn relative_to(&self, base: &Path) -> Option<PathBuf>;
}

impl PathUtil for Path {
    fn to_posix_string(&self) -> String {
        #[cfg(windows)]
        let path = self.to_string_lossy().replace('\\', "/");
        #[cfg(not(windows))]
        let path = self.to_string_lossy().to_string();
        path
    }

    fn relative_to(&self, base: &Path) -> Option<PathBuf> {
        let mut ita = self.components();
        let mut itb = base.components();
        let mut comps: Vec<Component> = vec![];

        loop {
            match (ita.next(), itb.next()) {
                (None, None) => break,
                (Some(a), None) => {
                    comps.push(a);
                    comps.extend(ita.by_ref());
                    break;
                }
                (None, _) => comps.push(Component::ParentDir),
                (Some(a), Some(b)) if comps.is_empty() && a == b => (),
                (Some(a), Some(Component::CurDir)) => comps.push(a),
                (Some(_), Some(Component::ParentDir)) => return None,
                (Some(a), Some(_)) => {
                    comps.push(Component::ParentDir);
                    for _ in itb {
                        comps.push(Component::ParentDir);
                    }
                    comps.push(a);
                    comps.extend(ita.by_ref());
                    break;
                }
            }
        }

        Some(comps.iter().map(|c| c.as_os_str()).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_to_posix_string() {
        #[cfg(windows)]
        let path = Path::new("foo\\bar\\baz");
        #[cfg(not(windows))]
        let path = Path::new("foo/bar/baz");
        assert_eq!(path.to_posix_string(), "foo/bar/baz");
    }

    #[test]
    fn test_relative_from() {
        let path = Path::new("foo/bar/baz");
        let base = Path::new("foo/bar");
        assert_eq!(path.relative_to(base), Some(PathBuf::from("baz")));

        let path = Path::new("foo/bar/baz");
        let base = Path::new("foo/bar/baz");
        assert_eq!(path.relative_to(base), Some(PathBuf::new()));

        let path = Path::new("foo/bar/baz");
        let base = Path::new("foo/bar/baz/qux");
        assert_eq!(path.relative_to(base), Some(PathBuf::from("..")));

        let path = Path::new("foo/bar/baz");
        let base = Path::new("foo/bar/qux");
        assert_eq!(path.relative_to(base), Some(PathBuf::from("../baz")));
    }
}
