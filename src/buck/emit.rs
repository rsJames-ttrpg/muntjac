//! Types and trait for the BUCK emitter.

use std::collections::BTreeMap;
use std::str::FromStr;

use crate::config::{Config, PythonVersion, Tree};
use crate::lock::types::{Lockfile, Wheel};
use crate::wheel::{PickResult, build_compatible_tags, pick_wheel};

#[derive(Debug, Clone)]
pub struct EmitInput {
    pub tree: String,
    pub third_party_dir: String,
    pub configs: Vec<ConfigName>,
    pub packages: Vec<EmitPackage>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EmitDeps {
    /// Every cell has the same dep list. Rendered as `deps = [":foo", ":bar"]`.
    Uniform(Vec<String>),

    /// Cells differ. Rendered as `deps = select({...})` with one branch per cell.
    /// Keys are every cell in `EmitInput::configs` (no `default` arm — cell
    /// coverage is exhaustive by construction in `build_emit_input`).
    PerCell(BTreeMap<ConfigName, Vec<String>>),
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

/// Compose the S1/S2 pipeline into an `EmitInput` for a single tree.
///
/// Walks each (platform, python) cell of the tree, picks a wheel per
/// package, and merges the per-cell selections into
/// `BTreeMap<ConfigName, EmitWheel>`. Output is deterministic: configs
/// are pre-sorted, wheels live in a BTreeMap, and packages are sorted
/// by (name, version).
///
/// Errors:
/// - `PickResult::NoWheel` for any (package, cell) — native sdists are
///   not handled until S5.
/// - Cross-cell dep-set mismatches for the same (name, version) —
///   per-cell `select()`-driven deps are deferred to S4.
pub fn build_emit_input(
    config: &Config,
    tree: &Tree,
    lockfile: &Lockfile,
) -> anyhow::Result<EmitInput> {
    let graph = crate::lock::graph::build(lockfile)?;
    crate::lock::graph::detect_cycles(&graph)?;
    let view = crate::lock::resolved::project(&graph, config, tree);

    // (name, version) -> &[Wheel] index from the lockfile. ResolvedPackage
    // carries no wheel data, so we cross-reference into the Lockfile here.
    let mut wheel_index: BTreeMap<(String, String), &[Wheel]> = BTreeMap::new();
    for pkg in &lockfile.packages {
        wheel_index.insert(
            (pkg.name.as_ref().to_string(), pkg.version.to_string()),
            pkg.wheels.as_slice(),
        );
    }

    // Sorted configs Vec for the EmitInput.
    let mut configs: Vec<ConfigName> = Vec::new();
    for plat_name in config.platforms.keys() {
        for py in &tree.python_versions {
            configs.push(ConfigName::new(&format!("{}.{}", py.0, py.1), plat_name));
        }
    }
    configs.sort();

    type PkgKey = (String, String);
    let mut pkg_wheels: BTreeMap<PkgKey, BTreeMap<ConfigName, EmitWheel>> = BTreeMap::new();
    let mut pkg_deps_per_cell: BTreeMap<PkgKey, BTreeMap<ConfigName, Vec<String>>> =
        BTreeMap::new();

    for resolved_cfg in &view.configs {
        let plat_name = &resolved_cfg.platform;
        let plat = config
            .platforms
            .get(plat_name)
            .ok_or_else(|| anyhow::anyhow!("platform `{}` missing from config", plat_name))?;
        let py =
            PythonVersion::from_str(&resolved_cfg.python_version).map_err(anyhow::Error::msg)?;
        let cfg_name = ConfigName::new(&resolved_cfg.python_version, plat_name);
        let compat = build_compatible_tags(plat, py.clone());

        for pkg in &resolved_cfg.packages {
            let key: PkgKey = (pkg.name.clone(), pkg.version.clone());
            let wheels = wheel_index.get(&key).copied().unwrap_or(&[]);
            if wheels.is_empty() {
                // First-party packages have no wheels; sdist-only registry
                // packages are out of scope until S5. Skip silently here so
                // happy path keeps working; S5 will revisit.
                continue;
            }
            match pick_wheel(wheels, &compat) {
                PickResult::Picked { wheel, .. } => {
                    pkg_wheels.entry(key.clone()).or_default().insert(
                        cfg_name.clone(),
                        EmitWheel {
                            url: wheel.url.to_string(),
                            hash: wheel.hash.clone(),
                        },
                    );
                    pkg_deps_per_cell
                        .entry(key)
                        .or_default()
                        .insert(cfg_name.clone(), pkg.deps.clone());
                }
                PickResult::NoWheel => {
                    anyhow::bail!(
                        "package '{}-{}' has no wheel for cell ({}, {}) and S3 does \
                         not yet handle native sdists. Restrict the affected \
                         python_versions or platforms in muntjac.toml until S5 lands.",
                        pkg.name,
                        pkg.version,
                        resolved_cfg.python_version,
                        plat_name
                    );
                }
            }
        }
    }

    // Cross-cell dep equality check + build EmitPackage list.
    let mut packages: Vec<EmitPackage> = Vec::new();
    for (key, wheel_map) in pkg_wheels {
        let cells_deps = &pkg_deps_per_cell[&key];
        let mut iter = cells_deps.iter();
        let (first_cell, first_deps) = iter
            .next()
            .expect("at least one cell recorded a wheel for this package");
        for (cell, deps) in iter {
            if deps != first_deps {
                anyhow::bail!(
                    "package '{}-{}' has different deps across cells:\n  {} -> {:?}\n  {} -> {:?}\n\
                     Per-cell select()-driven deps are deferred to S4.",
                    key.0,
                    key.1,
                    first_cell,
                    first_deps,
                    cell,
                    deps
                );
            }
        }
        // ResolvedPackage.deps entries are formatted "name@version" — extract
        // the bare package name for the BUCK target reference (":<name>").
        let mut deps: Vec<String> = first_deps
            .iter()
            .map(|d| {
                let name = d.split('@').next().unwrap_or(d);
                format!(":{}", name)
            })
            .collect();
        deps.sort();
        deps.dedup();
        packages.push(EmitPackage {
            name: key.0,
            version: key.1,
            deps,
            wheels: wheel_map,
        });
    }

    packages.sort_by(|a, b| a.name.cmp(&b.name).then(a.version.cmp(&b.version)));

    Ok(EmitInput {
        tree: tree.name.clone(),
        third_party_dir: tree.third_party_dir.to_string_lossy().into_owned(),
        configs,
        packages,
    })
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

    #[test]
    fn build_emit_input_from_synthetic_resolved() {
        use crate::config::{Config, Platform, PythonVersion, Tree};
        use crate::lock::types::{DepEdge, FirstPartyKind, Lockfile, Package, Source, Wheel};
        use pep440_rs::Version;
        use pep508_rs::PackageName;
        use std::str::FromStr;
        use url::Url;

        let tree = Tree {
            name: "default".into(),
            manifest_path: "pyproject.toml".into(),
            third_party_dir: "third-party/python".into(),
            python_versions: vec![PythonVersion(3, 12)],
        };

        let mut platforms = std::collections::BTreeMap::new();
        platforms.insert(
            "linux-x86_64-gnu".into(),
            Platform {
                target: "x86_64-unknown-linux-gnu".into(),
                manylinux: Some("2_17".into()),
                musllinux: None,
                macos_min: None,
            },
        );

        let config = Config {
            trees: vec![tree.clone()],
            platforms,
            fixups: Default::default(),
            buck: Default::default(),
            lockfile: Default::default(),
        };

        // A first-party root is required for the dep-graph projector to walk
        // reachable packages. Without it, `view.configs[i].packages` is empty
        // because roots are first-party-only.
        let lockfile = Lockfile {
            version: 1,
            revision: 3,
            requires_python: ">=3.12".into(),
            packages: vec![
                Package {
                    name: PackageName::from_str("app").unwrap(),
                    version: Version::from_str("0.1").unwrap(),
                    source: Source::FirstParty {
                        kind: FirstPartyKind::Virtual,
                        path: ".".into(),
                    },
                    dependencies: vec![DepEdge {
                        name: PackageName::from_str("certifi").unwrap(),
                        extra: vec![],
                        marker: None,
                    }],
                    sdist: None,
                    wheels: vec![],
                    metadata: None,
                },
                Package {
                    name: PackageName::from_str("certifi").unwrap(),
                    version: Version::from_str("2025.4.26").unwrap(),
                    source: Source::Registry {
                        url: Url::parse("https://pypi.org/simple").unwrap(),
                    },
                    dependencies: vec![],
                    sdist: None,
                    wheels: vec![Wheel {
                        url: Url::parse(
                            "https://files.pythonhosted.org/p/certifi-2025.4.26-py3-none-any.whl",
                        )
                        .unwrap(),
                        hash: "sha256:abc".into(),
                        size: None,
                        filename: "certifi-2025.4.26-py3-none-any.whl".into(),
                    }],
                    metadata: None,
                },
            ],
        };

        let input = build_emit_input(&config, &tree, &lockfile).expect("build_emit_input succeeds");

        assert_eq!(input.tree, "default");
        assert_eq!(input.third_party_dir, "third-party/python");
        assert_eq!(
            input.configs,
            vec![ConfigName::new("3.12", "linux-x86_64-gnu")]
        );
        assert_eq!(input.packages.len(), 1);
        let pkg = &input.packages[0];
        assert_eq!(pkg.name, "certifi");
        assert_eq!(pkg.version, "2025.4.26");
        assert!(pkg.deps.is_empty());
        assert_eq!(pkg.wheels.len(), 1);
        let wheel = pkg.wheels.values().next().unwrap();
        assert!(wheel.url.contains("certifi-2025.4.26"));
        assert_eq!(wheel.hash, "sha256:abc");
    }

    #[test]
    fn build_emit_input_errors_on_no_wheel() {
        use crate::config::{Config, Platform, PythonVersion, Tree};
        use crate::lock::types::{DepEdge, FirstPartyKind, Lockfile, Package, Source, Wheel};
        use pep440_rs::Version;
        use pep508_rs::PackageName;
        use std::str::FromStr;
        use url::Url;

        let tree = Tree {
            name: "default".into(),
            manifest_path: "pyproject.toml".into(),
            third_party_dir: "third-party/python".into(),
            python_versions: vec![PythonVersion(3, 12)],
        };
        let mut platforms = std::collections::BTreeMap::new();
        platforms.insert(
            "linux-x86_64-gnu".into(),
            Platform {
                target: "x86_64-unknown-linux-gnu".into(),
                manylinux: Some("2_17".into()),
                musllinux: None,
                macos_min: None,
            },
        );
        let config = Config {
            trees: vec![tree.clone()],
            platforms,
            fixups: Default::default(),
            buck: Default::default(),
            lockfile: Default::default(),
        };

        // First-party root depending on ancient-pkg, mirroring T11's pattern.
        // Without a first-party root the resolved-view projector returns empty
        // `view.configs[*].packages` and the NoWheel path is never reached.
        let app = Package {
            name: PackageName::from_str("app").unwrap(),
            version: Version::from_str("0.1.0").unwrap(),
            source: Source::FirstParty {
                kind: FirstPartyKind::Virtual,
                path: ".".into(),
            },
            dependencies: vec![DepEdge {
                name: PackageName::from_str("ancient-pkg").unwrap(),
                extra: vec![],
                marker: None,
            }],
            sdist: None,
            wheels: vec![],
            metadata: None,
        };
        let ancient = Package {
            name: PackageName::from_str("ancient-pkg").unwrap(),
            version: Version::from_str("0.1.0").unwrap(),
            source: Source::Registry {
                url: Url::parse("https://pypi.org/simple").unwrap(),
            },
            dependencies: vec![],
            sdist: None,
            wheels: vec![Wheel {
                url: Url::parse(
                    "https://example.com/ancient_pkg-0.1.0-cp310-cp310-manylinux_2_17_x86_64.whl",
                )
                .unwrap(),
                hash: "sha256:aaaa".into(),
                size: None,
                filename: "ancient_pkg-0.1.0-cp310-cp310-manylinux_2_17_x86_64.whl".into(),
            }],
            metadata: None,
        };

        let lockfile = Lockfile {
            version: 1,
            revision: 3,
            requires_python: ">=3.12".into(),
            packages: vec![app, ancient],
        };

        let err = build_emit_input(&config, &tree, &lockfile).expect_err("should fail on NoWheel");
        let msg = format!("{:#}", err);
        assert!(
            msg.contains("ancient-pkg"),
            "error must name the package: {}",
            msg
        );
        assert!(
            msg.contains("3.12"),
            "error must name the python version: {}",
            msg
        );
        assert!(
            msg.contains("linux-x86_64-gnu"),
            "error must name the platform: {}",
            msg
        );
        assert!(msg.contains("S5"), "error must point at S5: {}", msg);
    }

    #[test]
    fn build_emit_input_errors_on_dep_mismatch() {
        use crate::config::{Config, Platform, PythonVersion, Tree};
        use crate::lock::types::{DepEdge, FirstPartyKind, Lockfile, Package, Source, Wheel};
        use pep440_rs::Version;
        use pep508_rs::{MarkerTree, PackageName};
        use std::str::FromStr;
        use url::Url;

        let tree = Tree {
            name: "default".into(),
            manifest_path: "pyproject.toml".into(),
            third_party_dir: "third-party/python".into(),
            python_versions: vec![PythonVersion(3, 11), PythonVersion(3, 12)],
        };
        let mut platforms = std::collections::BTreeMap::new();
        platforms.insert(
            "linux-x86_64-gnu".into(),
            Platform {
                target: "x86_64-unknown-linux-gnu".into(),
                manylinux: Some("2_17".into()),
                musllinux: None,
                macos_min: None,
            },
        );
        let config = Config {
            trees: vec![tree.clone()],
            platforms,
            fixups: Default::default(),
            buck: Default::default(),
            lockfile: Default::default(),
        };

        // First-party root with a normal dep on `parent`.
        let app = Package {
            name: PackageName::from_str("app").unwrap(),
            version: Version::from_str("0.1.0").unwrap(),
            source: Source::FirstParty {
                kind: FirstPartyKind::Virtual,
                path: ".".into(),
            },
            dependencies: vec![DepEdge {
                name: PackageName::from_str("parent").unwrap(),
                extra: vec![],
                marker: None,
            }],
            sdist: None,
            wheels: vec![],
            metadata: None,
        };

        // `parent` has a marker-gated dep on `child` that fires only for py<3.12.
        // Result: parent's resolved deps differ across py3.11 vs py3.12 cells,
        // which the composer's cross-cell equality check must reject.
        let parent_marker = MarkerTree::from_str("python_version < '3.12'").expect("parse marker");
        let parent = Package {
            name: PackageName::from_str("parent").unwrap(),
            version: Version::from_str("1.0.0").unwrap(),
            source: Source::Registry {
                url: Url::parse("https://pypi.org/simple").unwrap(),
            },
            dependencies: vec![DepEdge {
                name: PackageName::from_str("child").unwrap(),
                extra: vec![],
                marker: Some(parent_marker),
            }],
            sdist: None,
            wheels: vec![Wheel {
                url: Url::parse("https://example.com/parent-1.0.0-py3-none-any.whl").unwrap(),
                hash: "sha256:pppp".into(),
                size: None,
                filename: "parent-1.0.0-py3-none-any.whl".into(),
            }],
            metadata: None,
        };

        let child = Package {
            name: PackageName::from_str("child").unwrap(),
            version: Version::from_str("1.0.0").unwrap(),
            source: Source::Registry {
                url: Url::parse("https://pypi.org/simple").unwrap(),
            },
            dependencies: vec![],
            sdist: None,
            wheels: vec![Wheel {
                url: Url::parse("https://example.com/child-1.0.0-py3-none-any.whl").unwrap(),
                hash: "sha256:cccc".into(),
                size: None,
                filename: "child-1.0.0-py3-none-any.whl".into(),
            }],
            metadata: None,
        };

        let lockfile = Lockfile {
            version: 1,
            revision: 3,
            requires_python: ">=3.11".into(),
            packages: vec![app, parent, child],
        };

        let err = build_emit_input(&config, &tree, &lockfile)
            .expect_err("should fail on cross-cell dep mismatch");
        let msg = format!("{:#}", err);
        // Error must name the package whose deps differ:
        assert!(
            msg.contains("parent"),
            "error must name the package: {}",
            msg
        );
        // Both cells named:
        assert!(
            msg.contains("py311") || msg.contains("3.11"),
            "error must name py3.11 cell: {}",
            msg
        );
        assert!(
            msg.contains("py312") || msg.contains("3.12"),
            "error must name py3.12 cell: {}",
            msg
        );
        // S4 reference:
        assert!(msg.contains("S4"), "error must point at S4: {}", msg);
    }

    #[test]
    fn emit_deps_variants_construct() {
        let uniform = EmitDeps::Uniform(vec![":foo".into(), ":bar".into()]);
        let per_cell = EmitDeps::PerCell({
            let mut m = BTreeMap::new();
            m.insert(
                ConfigName::new("3.12", "linux-x86_64-gnu"),
                vec![":foo".into()],
            );
            m.insert(
                ConfigName::new("3.11", "linux-x86_64-gnu"),
                vec![":foo".into(), ":bar".into()],
            );
            m
        });

        match uniform {
            EmitDeps::Uniform(v) => assert_eq!(v.len(), 2),
            EmitDeps::PerCell(_) => panic!("expected Uniform"),
        }
        match per_cell {
            EmitDeps::PerCell(m) => assert_eq!(m.len(), 2),
            EmitDeps::Uniform(_) => panic!("expected PerCell"),
        }
    }
}
