use std::collections::BTreeMap;
use std::io::Write;
use std::{fs::File, io::Read, path::Path};

use miette::{miette, IntoDiagnostic, WrapErr};

use crate::package::PackageStore;

use super::path_util::PathUtil;

/// Generate a Rust module for use by [tonic](https://docs.rs/tonic).
///
/// This function generates a Rust module that includes all the generated proxy/stub code
/// generated form the proto files in the package store.
///
/// # Example Module
///
/// ```no_run
/// // Generated using `buffrs install --generate-tonic-proto-module`
/// pub mod api {
///     pub mod examples {
///         pub mod hub {
///             // Package: api.examples.hub
///             // Path: ../proto/vendor/api-examples-hub/api/examples/hub/pkg.proto
///             include!(concat!(env!("OUT_DIR"), "/api.examples.hub.rs"));
///         }
///     }
/// }
/// pub mod lib {
///     pub mod geom {
///         // Package: lib.geom
///         // Path: ../proto/vendor/lib-geom/lib/geom/pkg.proto
///         include!(concat!(env!("OUT_DIR"), "/lib.geom.rs"));
///     }
/// }
/// ```
///
/// # Arguments
/// * `store` - The package store
/// * `module_path` - Desired path to the generated module
/// * `tonic_out_dir` - The output directory for the generated tonic code
pub async fn generate_tonic_proto_module(
    store: &PackageStore,
    module_path: &Path,
    tonic_out_dir: Option<&str>,
) -> miette::Result<()> {
    // Resolve the output directory
    let module_dir = module_path
        .parent()
        .and_then(|dir| {
            if dir.components().count() > 0 {
                Some(dir)
            } else {
                None
            }
        })
        .map_or_else(std::env::current_dir, |dir| Ok(dir.to_path_buf()))
        .into_diagnostic()
        .wrap_err_with(|| {
            miette::diagnostic!(
            "invalid module path specified with `buffrs install --generate-tonic-proto-module {}`",
            module_path.display()
        )
        })?;

    // Convert the module path to absolute
    let module_path = module_dir.join(module_path.file_name().unwrap_or_default());

    std::fs::create_dir_all(&module_dir).into_diagnostic()?;

    // Collect all dependent packages
    let packages = collect_dependent_packages(store, &module_dir).await?;

    // Group the packages by their hierarchical components
    let tree = create_module_tree(packages);

    // Generate the module file
    let file = File::create(&module_path)
        .into_diagnostic()
        .wrap_err(miette!(
            "failed to create tonic-proto module file \"{}\"",
            module_path.display()
        ))?;

    generate_proto_module(file, tree, tonic_out_dir)?;

    Ok(())
}

/// Create a module tree, a pre-requisite for generating the tonic proto module.
fn create_module_tree(packages: Vec<ProtoPackage>) -> BTreeMap<String, Value> {
    let mut root = BTreeMap::new();
    for package in packages {
        let mut current = &mut root;
        for component in &package.package_components {
            let node = current
                .entry(component.clone())
                .or_insert_with(|| Value::Children(BTreeMap::new()));
            current = match node {
                Value::Children(children) => children,
                _ => unreachable!(),
            };
        }

        let key = format!("__{}", package.name);
        current.insert(key, Value::Package(package));
    }
    root
}

/// Collect all dependent packages from the package store.
async fn collect_dependent_packages(
    store: &PackageStore,
    module_dir: &Path,
) -> miette::Result<Vec<ProtoPackage>> {
    // Get all files under the control of the store
    let paths = store
        .collect(&store.proto_path(), false)
        .await
        .into_iter()
        .chain(
            store
                .collect(&store.proto_vendor_path(), true)
                .await
                .into_iter(),
        );

    let mut packages = Vec::new();
    for path in paths {
        let package_name = extract_package_name(&path)?;
        let package_components = package_name.split('.').map(str::to_string).collect();

        let abs_module_dir = module_dir
            .canonicalize()
            .into_diagnostic()
            .wrap_err(miette!(
                "failed to canonicalize module directory \"{}\"",
                module_dir.display()
            ))?;
        let rel_path = path
            .canonicalize()
            .into_diagnostic()
            .wrap_err(miette!(
                "failed to canonicalize proto path \"{}\"",
                path.display()
            ))?
            .relative_to(&abs_module_dir)
            .unwrap_or(path.clone())
            .to_posix_string();

        packages.push(ProtoPackage {
            name: package_name,
            path: rel_path,
            package_components,
        });
    }
    Ok(packages)
}

/// Extract the package name from a proto file.
///
/// This function reads the contents of the proto file and extracts the package name from it
/// by looking for the `package` keyword. If the package name is not found, then an empty string
/// is returned.
///
/// # Arguments
/// * `proto_file` - The path to the proto file
fn extract_package_name(proto_file: &Path) -> miette::Result<String> {
    let mut contents = String::new();
    File::open(proto_file)
        .into_diagnostic()?
        .read_to_string(&mut contents)
        .into_diagnostic()?;

    Ok(contents
        .lines()
        .find_map(|line| {
            line.trim_start()
                .strip_prefix("package ")
                .map(|package| package.trim_end_matches(';').to_string())
        })
        .unwrap_or_default())
}

/// Render the module tree to a file.
fn generate_proto_module(
    mut file: File,
    tree: BTreeMap<String, Value>,
    tonic_out_dir: Option<&str>,
) -> miette::Result<()> {
    // Write the header
    writeln!(
        file,
        "// Generated using `buffrs install --generate-tonic-proto-module`\n"
    )
    .into_diagnostic()?;

    render_node(&mut file, &tree, 0, tonic_out_dir)?;

    fn write_line<W: Write>(file: &mut W, indent_level: usize, line: &str) -> miette::Result<()> {
        writeln!(file, "{}{}", "    ".repeat(indent_level), line).into_diagnostic()
    }

    // Render tree (depth-first)
    fn render_node(
        file: &mut File,
        tree: &BTreeMap<String, Value>,
        level: usize,
        tonic_out_dir: Option<&str>,
    ) -> miette::Result<()> {
        for (key, value) in tree {
            match value {
                Value::Package(package) => {
                    write_line(file, level, &format!("// Package: {}", package.name))?;
                    write_line(file, level, &format!("// Path: {}", package.path))?;

                    match tonic_out_dir {
                        Some(out_dir) => {
                            write_line(
                                file,
                                level,
                                &format!(
                                    "include!(concat!({}, \"/{}.rs\"));",
                                    out_dir, package.name
                                ),
                            )?;
                        }
                        None => {
                            write_line(
                                file,
                                level,
                                &format!("tonic::include_proto!(\"{}\");", package.name),
                            )?;
                        }
                    }
                }
                Value::Children(children) => {
                    write_line(file, level, &format!("pub mod {} {{", key))?;
                    render_node(file, children, level + 1, tonic_out_dir)?;
                    writeln!(file, "{}}}", "    ".repeat(level)).into_diagnostic()?;
                }
            }
        }
        Ok(())
    }

    Ok(())
}

#[derive(Debug)]
struct ProtoPackage {
    name: String,
    path: String,
    package_components: Vec<String>,
}

type Key = String;
#[derive(Debug)]
enum Value {
    Package(ProtoPackage),
    Children(BTreeMap<Key, Value>),
}
