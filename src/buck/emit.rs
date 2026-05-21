//! Types and trait for the BUCK emitter.

use std::collections::BTreeMap;

#[derive(Debug, Clone)]
pub struct EmitInput {
    pub tree: String,
    pub third_party_dir: String,
    pub configs: Vec<ConfigName>,
    pub packages: Vec<EmitPackage>,
}

#[derive(Debug, Clone)]
pub struct EmitPackage {
    pub name: String,
    pub version: String,
    pub deps: Vec<String>,
    pub wheels: BTreeMap<ConfigName, EmitWheel>,
}

#[derive(Debug, Clone)]
pub struct EmitWheel {
    pub url: String,
    pub hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ConfigName(String);

impl ConfigName {
    /// Build a config name from a Python version string (e.g. "3.12") and a
    /// platform key (e.g. "linux-x86_64-gnu"). Result: "py312-linux-x86_64-gnu".
    pub fn new(py_version: &str, platform_name: &str) -> Self {
        let mut s = String::with_capacity(8 + platform_name.len());
        s.push_str("py");
        for c in py_version.chars().filter(|c| c.is_ascii_digit()) {
            s.push(c);
        }
        s.push('-');
        s.push_str(platform_name);
        ConfigName(s)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for ConfigName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone)]
pub struct EmitOutput {
    pub buck: String,
    pub muntjac_bzl: String,
    pub config_buck: String,
    pub package_file: String,
}

/// Trait for muntjac's BUCK emitter. The v1 implementation is
/// `StringTemplateEmitter` (hand-rolled writeln! formatting). Future
/// implementations (typed-AST or template-engine) can plug in without
/// changing the CLI or pipeline composer.
///
/// Implementations MUST be deterministic: same input -> same byte output.
/// All map iteration must use BTreeMap or pre-sorted Vec.
pub trait BuckEmitter {
    fn emit(&self, input: &EmitInput) -> EmitOutput;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn emit_input_constructs() {
        let inp = EmitInput {
            tree: "default".into(),
            third_party_dir: "third-party/python".into(),
            configs: vec![ConfigName::new("3.12", "linux-x86_64-gnu")],
            packages: vec![EmitPackage {
                name: "requests".into(),
                version: "2.32.3".into(),
                deps: vec![":certifi".into(), ":idna".into()],
                wheels: {
                    let mut m = BTreeMap::new();
                    m.insert(
                        ConfigName::new("3.12", "linux-x86_64-gnu"),
                        EmitWheel {
                            url: "https://example.com/requests-2.32.3-py3-none-any.whl".into(),
                            hash: "sha256:abc".into(),
                        },
                    );
                    m
                },
            }],
        };
        assert_eq!(inp.tree, "default");
        assert_eq!(inp.packages.len(), 1);
        assert_eq!(inp.configs[0].as_str(), "py312-linux-x86_64-gnu");
    }

    #[test]
    fn config_name_orders_lexicographically() {
        let a = ConfigName::new("3.11", "linux-x86_64-gnu");
        let b = ConfigName::new("3.12", "linux-x86_64-gnu");
        assert!(a < b);
        assert_eq!(a.as_str(), "py311-linux-x86_64-gnu");
    }
}
