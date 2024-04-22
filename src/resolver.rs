use std::{
    collections::{HashMap, HashSet},
    fmt::Display,
};

use futures::executor::block_on;
use miette::{miette, Context, IntoDiagnostic};
use pubgrub::solver::DependencyProvider;
use semver::Version;
use url::Url;

use crate::{
    cache::Cache,
    credentials::Credentials,
    lock::{LockedPackage, Lockfile},
    manifest::{Dependency, DependencyManifest, Manifest},
    package::PackageName,
    registry::{Artifactory, RegistryUri},
};

//#[derive(Error, Diagnostic, Debug)]
//#[error("failed to download dependency {name}@{version} from the registry")]
//struct DownloadError {
//name: PackageName,
//version: VersionReq,
//}

pub struct Resolver<'c, 'l> {
    /// The input graph to resolve
    input: HashMap<PackageName, DependencyManifest>,
    credentials: Credentials,
    cache: &'c Cache,
    lockfile: &'l Lockfile,
}

impl<'c, 'l> Resolver<'c, 'l> {
    pub fn new(
        manifest: &Manifest,
        credentials: Credentials,
        cache: &'c Cache,
        lockfile: &'l Lockfile,
    ) -> Self {
        let input = manifest
            .dependencies
            .iter()
            .cloned()
            .map(|manifest| (manifest.package, manifest.manifest))
            .collect();

        Self {
            input,
            credentials,
            cache,
            lockfile,
        }
    }

    pub async fn resolve(self) -> miette::Result<HashSet<LockedPackage>> {
        // Version Resolution
        //
        // Manifest -> HashSet<UnresolvedDependency> -> Resolver
        //
        // Cache -> Lockfile -> Resolver -> HashSet<ResolvedDependency>
        //
        // Resolving:
        //
        // 1. USE AS MUCH LOCKFILE INFORMATION AS POSSIBLE TO POPULATE RESOLVER STATE
        // 2. LOAD MISSING INFORMATION (e.g. new deps) FROM ARTIFACTORY AND ADD TO RESOLVER STATE
        // 3. RUN PUBGUB AND REPEAT 2 UNTIL TERMINATED
        // 4. REPLACE LOCKFILE WITH RESULT
        // 5. RETURN Vec<ResolvedDependency> for Installation

        #[derive(Default)]
        struct PubGrubResolver {
            state: HashMap<PackageName, LockedPackage>,
        }

        let mut pubgrub = PubGrubResolver::default();

        for (name, manifest) in self.input {
            if let Some(locked) = self.lockfile.get(&name).cloned() {
                pubgrub.state.insert(name, locked);
                continue;
            }

            let artifactory = Artifactory::new(manifest.registry.clone(), &self.credentials)?;

            let dependency = Dependency {
                package: name.clone(),
                manifest: manifest.clone(),
            };

            let package = artifactory.download(dependency, Some(&self.cache)).await?;

            let locked = LockedPackage::lock(&package, manifest.registry, manifest.repository);

            pubgrub.state.insert(name, locked);
        }

        use pubgrub::error::PubGrubError;
        use pubgrub::report::{DefaultStringReporter, Reporter};
        use pubgrub::solver::resolve;

        let provider = Provider {
            credentials: &self.credentials,
            cache: self.cache,
            lockfile: self.lockfile,
        };

        let root_package = DependencyLocator(
            RegistryUri::unchecked("file://.".parse().into_diagnostic()?),
            "unknown".to_string(),
            PackageName::unchecked("."),
        );
        let root_version = DependencyVersion(Version::new(0, 0, 0));

        let deps = match resolve(&provider, root_package, root_version) {
            Ok(solution) => solution,
            Err(PubGrubError::NoSolution(mut derivation_tree)) => {
                derivation_tree.collapse_no_versions();
                panic!("{}", DefaultStringReporter::report(&derivation_tree));
            }
            Err(err) => panic!("{:?}", err),
        };

        Ok(HashSet::default())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct DependencyLocator(RegistryUri, String, PackageName);

impl Display for DependencyLocator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", self.1, self.2)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct DependencyVersion(semver::Version);

impl pubgrub::version::Version for DependencyVersion {
    fn bump(&self) -> Self {
        Self(semver::Version::new(
            self.0.major,
            self.0.minor,
            self.0.patch + 1,
        ))
    }

    fn lowest() -> Self {
        Self(semver::Version::new(0, 0, 0))
    }
}

impl Display for DependencyVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Statically known information provider about the current package
struct RootProvider {
    locator: DependencyLocator,
    version: DependencyVersion,
    dependencies: Vec<Dependency>,
}

struct Provider<'a, 'c, 'l> {
    root: RootProvider,
    credentials: &'a Credentials,
    cache: &'c Cache,
    lockfile: &'l Lockfile,
}

impl<'a, 'c, 'l> Provider<'a, 'c, 'l> {
    fn available_versions(
        &self,
        dependency: &DependencyLocator,
    ) -> miette::Result<Vec<DependencyVersion>> {
        // 1. If we are asked for available versions of the local package we return only one
        if &self.root.locator == dependency {
            return Ok(vec![self.root.version.clone()]);
        }

        let artifactory = Artifactory::new(dependency.0.clone(), &self.credentials).unwrap();

        let versions = futures::executor::block_on(
            artifactory.versions(dependency.2.clone(), dependency.1.clone()),
        )?;

        Ok(versions.into_iter().map(DependencyVersion).collect())
    }
}

//impl<'a, 'c, 'l> DependencyProvider<DependencyLocator, DependencyVersion> for Provider<'a, 'c, 'l> {
//fn get_dependencies(
//&self,
//package: &DependencyLocator,
//version: &DependencyVersion,
//) -> Result<
//pubgrub::solver::Dependencies<DependencyLocator, DependencyVersion>,
//Box<dyn std::error::Error>,
//> {
//// 0. Return statically known dependency information if possible (root package only)

//if &self.root.locator == package {
//if &self.root.version != version {
//return Err(
//miette!("unable to resolve local package at a different version").into(),
//);
//}

//return Ok(pubgrub::solver::Dependencies::Known(self.root.)
//}

//// 1. Retrieve dependencies from lockfile
//// 2. Open cached version
//// 3. Make request to artifactory

//let dependency = Dependency {
//package: package.2.clone(),
//manifest: DependencyManifest {
//version: format!("={}", version.0).parse()?,
//repository: package.1.clone(),
//registry: package.0.clone(),
//},
//};

//let artifactory = Artifactory::new(dependency.manifest.registry.clone(), &self.credentials)
//.wrap_err(miette!("failed to connect to artifactory"))?;

//let package = block_on(artifactory.download(dependency, Some(&self.cache)))
//.wrap_err(miette!("failed to resolve dependency using artifactory"))?;

//let dependencies: pubgrub::type_aliases::Map<
//DependencyLocator,
//pubgrub::range::Range<DependencyVersion>,
//> = package
//.manifest
//.dependencies
//.into_iter()
//.map(|d| {
//let locator =
//DependencyLocator(d.manifest.registry, d.manifest.repository, d.package);

//let version = pubgrub::range::Range::any();

//(locator, version)
//})
//.collect();

//Ok(pubgrub::solver::Dependencies::Known(dependencies))
//}

//fn choose_package_version<
//T: std::borrow::Borrow<DependencyLocator>,
//U: std::borrow::Borrow<pubgrub::range::Range<DependencyVersion>>,
//>(
//&self,
//mut potential_packages: impl Iterator<Item = (T, U)>,
//) -> Result<(T, Option<DependencyVersion>), Box<dyn std::error::Error>> {
//let (package, range) = potential_packages.next().ok_or(miette!(
//"failed to choose package version while resolving versions"
//))?;

//let pkg: &DependencyLocator = package.borrow();

//// 1. If we have this exact package present in our lockfile, lets resolve to its version
//if let Some(locked) = self.lockfile.get(&pkg.2) {
//// 1.1 Verify that not only the name, but the full locator matches
//if locked.registry == pkg.0 && locked.repository == pkg.1 {
//return Ok((package, Some(DependencyVersion(locked.version.clone()))));
//}
//}

//// 2. Get all possible versions
//let versions = self.available_versions(package.borrow())?;

//// 3. Select first version that matches the range
//let version = versions
//.into_iter()
//.filter(|v| range.borrow().contains(v))
//.next();

//Ok((package, version))
//}
//}
