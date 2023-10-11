use super::*;

/// Packages that make up a protocol buffer package.
#[derive(Serialize, Deserialize, Default, Clone, Debug, PartialEq, Eq, Diff)]
#[diff(attr(
    #[derive(Debug)]
    #[allow(missing_docs)]
))]
pub struct Packages {
    /// Packages defined in this protocol buffer package.
    pub packages: BTreeMap<String, Package>,
}

/// Error parsing packages.
#[derive(Error, Debug, Diagnostic)]
#[allow(missing_docs)]
pub enum PackagesError {
    #[error("duplicate package {package}, defined in {previous} and {current}")]
    #[diagnostic(
        help = "check to make sure your files define different package names",
        code = "duplicate_package"
    )]
    DuplicatePackage {
        package: String,
        current: PathBuf,
        previous: PathBuf,
    },

    #[error("error parsing package {package} in {file}")]
    Package {
        package: String,
        file: String,
        #[source]
        #[diagnostic_source]
        error: PackageError,
    },
}

impl Packages {
    /// Add a package from a [`FileDescriptorProto`].
    pub fn add(&mut self, descriptor: &FileDescriptorProto) -> Result<(), PackagesError> {
        let name = descriptor.package().to_string();
        let package = Package::new(descriptor).map_err(|error| PackagesError::Package {
            package: descriptor.package().to_string(),
            file: descriptor.name().to_string(),
            error,
        })?;
        match self.packages.entry(name) {
            Entry::Vacant(entry) => {
                entry.insert(package);
                Ok(())
            }
            Entry::Occupied(entry) => Err(PackagesError::DuplicatePackage {
                package: descriptor.package().to_string(),
                previous: entry.get().file.clone(),
                current: package.file.clone(),
            }),
        }
    }

    /// Generate a diff between two parsed [`Packages`].
    pub fn diff(&self, other: &Self) -> <Self as Diff>::Repr {
        Diff::diff(self, other)
    }

    /// Run checks against this.
    pub fn check(&self, rules: &mut RuleSet) -> Violations {
        let mut violations = rules.check_packages(self);
        for package in self.packages.values() {
            violations.append(&mut package.check(rules));
        }
        violations
    }
}
