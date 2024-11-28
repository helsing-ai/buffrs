use miette::{miette, Context, IntoDiagnostic};
use pretty_yaml::config::FormatOptions;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, fs, io::Write, path::Path};
use walkdir::WalkDir;

use crate::{manifest::Manifest, package::PackageStore, resolver::DependencyGraph};

const BUF_YAML_FILE: &str = "buf.yaml";

/// Representation of a Buf YAML file
pub struct BufYamlFile {
    config: Config,
}

impl BufYamlFile {
    /// Create default `BufYamlFile` with default values
    pub fn new() -> Self {
        let config: Config = serde_yml::from_str(DEFAULT_YAML).unwrap();
        Self { config }
    }

    /// Create a new `BufYamlFile` from a string
    pub fn new_from_str(s: &str) -> miette::Result<Self> {
        let config: Config = serde_yml::from_str(s).into_diagnostic()?;

        // Version needs to be v2
        if config.version != "v2" {
            return Err(miette!("Only v2 is supported for Buf YAML files"));
        }

        Ok(Self { config })
    }

    /// Serialize the `BufYamlFile` to a string
    pub fn to_string(&self) -> miette::Result<String> {
        let yaml = serde_yml::to_string(&self.config).into_diagnostic()?;

        // prettyfy the output
        let options = FormatOptions::default();
        pretty_yaml::format_text(&yaml, &options).into_diagnostic()
    }

    /// Load `BufYamlFile` from a YAML file
    pub fn from_file() -> miette::Result<Self> {
        let yaml_content = fs::read_to_string(BUF_YAML_FILE).into_diagnostic()?;
        Self::new_from_str(&yaml_content)
    }

    /// Write `BufYamlFile` to a YAML file
    pub fn to_file(&self) -> miette::Result<()> {
        let yaml_content = self.to_string()?;
        let mut file = fs::File::create(BUF_YAML_FILE).into_diagnostic()?;
        file.write_all(yaml_content.as_bytes()).into_diagnostic()?;
        Ok(())
    }

    /// Clear all modules from the Buf YAML file
    pub fn clear_modules(&mut self) {
        self.config.modules.clear();
    }

    /// Add non-vendor module to the Buf YAML file
    pub fn add_module(&mut self) {
        let proto_path = PackageStore::PROTO_PATH.to_string();
        let proto_vendor_path = PackageStore::PROTO_VENDOR_PATH.to_string();

        self.config.modules.push(Module {
            path: proto_path,
            excludes: vec![proto_vendor_path],
            ..Default::default()
        });
    }

    /// Add vendor modules to the Buf YAML file
    pub fn set_vendor_modules(&mut self, vendor_modules: Vec<String>) {
        let proto_vendor_path = PackageStore::PROTO_VENDOR_PATH.to_string();

        // Add vendor modules
        for module in vendor_modules {
            self.config.modules.push(Module {
                path: format!("{}/{}", &proto_vendor_path, module),
                ..Default::default()
            });
        }
    }
}

impl Default for BufYamlFile {
    fn default() -> Self {
        Self::new()
    }
}

/// Generates a buf.yaml file matching the current dependency graph
pub fn generate_buf_yaml_file(
    dependency_graph: &DependencyGraph,
    manifest: &Manifest,
    store: &PackageStore,
) -> Result<(), miette::Error> {
    let mut buf_yaml = if Path::new("buf.yaml").exists() {
        BufYamlFile::from_file().wrap_err(miette!("failed to read buf.yaml file"))?
    } else {
        BufYamlFile::default()
    };
    let mut vendor_modules: Vec<String> = dependency_graph
        .get_package_names()
        .iter()
        .map(|p| p.to_string())
        .collect();
    vendor_modules.sort();
    buf_yaml.clear_modules();
    if manifest.package.is_some() {
        // double-check that the package really contains proto files
        // under proto/** (but not under proto/vendor/**)
        let vendor_path = store.proto_vendor_path();
        let mut has_protos = false;
        for entry in WalkDir::new(store.proto_path()).into_iter().flatten() {
            if entry.path().is_file() {
                let path = entry.path();
                if path.starts_with(&vendor_path) {
                    continue;
                }

                if path.extension().map_or(false, |ext| ext == "proto") {
                    has_protos = true;
                    break;
                }
            }
        }

        if has_protos {
            buf_yaml.add_module();
        }
    }
    buf_yaml.set_vendor_modules(vendor_modules);
    buf_yaml
        .to_file()
        .wrap_err(miette!("failed to write buf.yaml file"))?;
    Ok(())
}

/// Default buf.yaml file
const DEFAULT_YAML: &str = r#"
version: v2

modules:
lint:
  use:
    - DEFAULT
  except:
    - PACKAGE_VERSION_SUFFIX
breaking:
  use:
    - FILE
deps:
  - buf.build/googleapis/googleapis
  - buf.build/grpc/grpc
"#;

#[derive(Debug, Serialize, Deserialize)]
struct Config {
    version: String,
    modules: Vec<Module>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    deps: Vec<String>,
    lint: Option<LintConfig>,
    breaking: Option<BreakingConfig>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    plugins: Vec<Plugin>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
struct Module {
    path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    excludes: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    includes: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    lint: Option<LintConfig>,
    #[serde(default, skip_serializing_if = "is_false")]
    disallow_comment_ignores: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    enum_zero_value_suffix: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    rpc_allow_same_request_response: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    rpc_allow_google_protobuf_empty_requests: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    rpc_allow_google_protobuf_empty_responses: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    service_suffix: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    disable_builtin: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    breaking: Option<BreakingConfig>,
}

#[derive(Debug, Serialize, Deserialize)]
struct LintConfig {
    #[serde(rename = "use", default, skip_serializing_if = "Vec::is_empty")]
    use_rules: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    except: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    ignore: Vec<String>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    ignore_only: HashMap<String, Vec<String>>,
}

#[derive(Debug, Serialize, Deserialize)]
struct BreakingConfig {
    #[serde(rename = "use", default, skip_serializing_if = "Vec::is_empty")]
    use_rules: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    except: Vec<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    ignore_unstable_packages: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    disable_builtin: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct Plugin {
    plugin: String,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    options: HashMap<String, String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct PluginOptions {
    timestamp_suffix: Option<String>,
}

fn is_false(b: impl std::borrow::Borrow<bool>) -> bool {
    !b.borrow()
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    #[test]
    fn test_gen_default_buf_yaml() {}

    #[test]
    fn test_new_from_str() {
        let buf_yaml = BufYamlFile::new_from_str(SAMPLE_YAML).unwrap();

        // serialize to file
        let serialized = buf_yaml.to_string().unwrap();

        let writer = std::fs::File::create("buf.yaml").unwrap();
        let mut writer = std::io::BufWriter::new(writer);
        writer.write_all(serialized.as_bytes()).unwrap();
    }

    const SAMPLE_YAML: &str = r#"
version: v2
modules:
  - path: proto/foo
    name: buf.build/acme/foo
  - path: proto/bar
    name: buf.build/acme/bar
    excludes:
      - proto/bar/a
      - proto/bar/b/e.proto
    lint:
      use:
        - STANDARD
      except:
        - IMPORT_USED
      ignore:
        - proto/bar/c
      ignore_only:
        ENUM_ZERO_VALUE_SUFFIX:
          - proto/bar/a
          - proto/bar/b/f.proto
    disallow_comment_ignores: false
    enum_zero_value_suffix: _UNSPECIFIED
    rpc_allow_same_request_response: false
    rpc_allow_google_protobuf_empty_requests: false
    rpc_allow_google_protobuf_empty_responses: false
    service_suffix: Service
    disable_builtin: false
    breaking:
      use:
        - FILE
      except:
        - EXTENSION_MESSAGE_NO_DELETE
      ignore_unstable_packages: true
      disable_builtin: false
  - path: proto/common
    module: buf.build/acme/weather
    includes:
      - proto/common/weather
  - path: proto/common
    module: buf.build/acme/location
    includes:
      - proto/common/location
    excludes:
      - proto/common/location/test
  - path: proto/common
    module: buf.build/acme/other
    excludes:
      - proto/common/location
      - proto/common/weather
deps:
  - buf.build/acme/paymentapis
  - buf.build/acme/pkg:47b927cbb41c4fdea1292bafadb8976f
  - buf.build/googleapis/googleapis:v1beta1.1.0
lint:
  use:
    - STANDARD
    - TIMESTAMP_SUFFIX
breaking:
  use:
    - FILE
plugins:
  - plugin: plugin-timestamp-suffix
    options:
      timestamp_suffix: _time
"#;
}
