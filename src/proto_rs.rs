use miette::IntoDiagnostic;
use std::collections::BTreeMap;
use std::io::Write;
use std::path::PathBuf;
use std::{fs::File, io::Read, path::Path};

use crate::package::PackageStore;

/// Generate the `proto.rs` file in the `src` directory.
///
/// This function generates the `proto.rs` file in the `src` directory. The `proto.rs` file
/// contains a module for each package in the `proto` directory. Each module includes the
/// generated Rust code for the proto files in the package.
///
/// # Arguments
/// * `store` - The package store
/// * `out_dir` - The output directory
pub async fn generate_proto_rs_file(store: &PackageStore, out_dir: &Path) -> miette::Result<()> {
    // Make sure the output directory exists
    std::fs::create_dir_all(out_dir).into_diagnostic()?;

    #[derive(Debug)]
    struct ProtoPackage {
        name: String,
        path: String,
        package_components: Vec<String>,
    }

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
        let rel_path = path_relative_from(&path, out_dir)
            .unwrap_or(path.clone())
            .to_string_lossy()
            .to_string();
        #[cfg(windows)]
        let rel_path = rel_path.replace("\\", "/");

        packages.push(ProtoPackage {
            name: package_name,
            path: rel_path,
            package_components,
        });
    }

    // Group the packages by their hierarchical components
    type Key = String;
    #[derive(Debug)]
    enum Value {
        Package(ProtoPackage),
        Children(BTreeMap<Key, Value>),
    }

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

    // Generate the `proto.rs` file in the src directory
    let mut file = File::create(out_dir.join("proto.rs")).into_diagnostic()?;

    // Write the `proto.rs` file header
    writeln!(file, "// Generated using `buffrs install --proto-rs`\n").into_diagnostic()?;

    fn write_line<W: Write>(file: &mut W, indent_level: usize, line: &str) -> miette::Result<()> {
        writeln!(file, "{}{}", "    ".repeat(indent_level), line).into_diagnostic()
    }

    // Render tree (depth-first)
    fn render_tree(
        file: &mut File,
        tree: &BTreeMap<String, Value>,
        level: usize,
    ) -> miette::Result<()> {
        for (key, value) in tree {
            match value {
                Value::Package(package) => {
                    write_line(file, level, &format!("// Package: {}", package.name))?;
                    write_line(file, level, &format!("// Path: {}", package.path))?;
                    write_line(
                        file,
                        level,
                        &format!(
                            "include!(concat!(env!(\"OUT_DIR\"), \"/{}.rs\"));",
                            package.name
                        ),
                    )?;
                }
                Value::Children(children) => {
                    write_line(file, level, &format!("pub mod {} {{", key))?;
                    render_tree(file, children, level + 1)?;
                    writeln!(file, "{}}}", "    ".repeat(level)).into_diagnostic()?;
                }
            }
        }
        Ok(())
    }

    // Write the nested modules
    render_tree(&mut file, &root, 0)?;

    Ok(())
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

/// Get the relative path of `path` from `base`.
fn path_relative_from(path: &Path, base: &Path) -> Option<PathBuf> {
    use std::path::Component;

    // Resolve `base` to an absolute path if it's relative
    let base = if base.is_absolute() {
        base.to_path_buf()
    } else {
        std::env::current_dir().ok()?.join(base).canonicalize().ok()?
    };

    // Resolve `path` to an absolute path if it's relative
    let path = std::env::current_dir().ok()?.join(path).canonicalize().ok()?;

    let mut ita = path.components();
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
