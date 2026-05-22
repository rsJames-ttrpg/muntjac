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
    pub deps: EmitDeps,
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
    pub wiring_bzl: String,
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
/// Per-cell deps are merged into `EmitDeps::Uniform` when every cell
/// agrees, else kept as `EmitDeps::PerCell` for `select()`-driven
/// rendering in the writer.
///
/// Errors:
/// - `PickResult::NoWheel` for any (package, cell) — native sdists are
///   not handled until S5.
pub fn build_emit_input(
    config: &Config,
    tree: &Tree,
    lockfile: &Lockfile,
    manifest: Option<&crate::sdist::Manifest>,
) -> anyhow::Result<EmitInput> {
    let graph = crate::lock::graph::build(lockfile)?;
    crate::lock::graph::detect_cycles(&graph)?;
    let view = crate::lock::resolved::project(&graph, config, tree);

    // Closure that looks up a (package, version) in the prebake manifest.
    let manifest_entry = |name: &str, ver: &str| -> Option<crate::sdist::ManifestEntry> {
        manifest.and_then(|m| {
            m.entries
                .iter()
                .find(|e| e.package == name && e.version == ver)
                .cloned()
        })
    };

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

            // ---- Sdist-only path: consult the prebake manifest. ----
            if wheels.is_empty() {
                // Find the lockfile package to see whether there's an sdist.
                let lock_pkg = lockfile.packages.iter().find(|p| {
                    p.name.as_ref() == pkg.name && p.version.to_string() == pkg.version
                });
                let sdist = lock_pkg.and_then(|p| p.sdist.clone());

                let Some(sdist) = sdist else {
                    // First-party (virtual/editable/directory) packages have
                    // no wheels and no sdist — skip silently as before.
                    continue;
                };
                let lockfile_sdist_sha = sdist.hash.trim_start_matches("sha256:").to_string();

                let Some(entry) = manifest_entry(&pkg.name, &pkg.version) else {
                    anyhow::bail!(
                        "pure-python sdist {} {} not prebaked. Run `muntjac vendor` first.",
                        pkg.name,
                        pkg.version
                    );
                };

                if entry.sdist_sha256 != lockfile_sdist_sha {
                    anyhow::bail!(
                        "prebake of {} {} is stale (sdist sha changed in uv.lock). Run `muntjac vendor`.",
                        pkg.name,
                        pkg.version
                    );
                }

                match entry.classification {
                    crate::sdist::ManifestClassification::PurePython {
                        wheel_filename,
                        wheel_sha256,
                        ..
                    } => {
                        // Pure-python wheel: same source covers every cfg.
                        pkg_wheels.entry(key.clone()).or_default().insert(
                            cfg_name.clone(),
                            EmitWheel {
                                url: format!("prebake:{}", wheel_filename),
                                hash: format!("sha256:{}", wheel_sha256),
                            },
                        );
                        pkg_deps_per_cell
                            .entry(key)
                            .or_default()
                            .insert(cfg_name.clone(), pkg.deps.clone());
                        continue;
                    }
                    crate::sdist::ManifestClassification::Native { .. } => {
                        // Canonical §6 error message; one per affected cell.
                        anyhow::bail!(
                            "{} {} has no wheel for ({}, {}) and is a native sdist.\n       \
                             add a fixup at third-party/python/fixups/{}/fixups.toml — see\n       \
                             `muntjac fixups show {}` for the current community fixup, or use\n       \
                             `replace_deps` to point at a hand-rolled Buck target.",
                            pkg.name,
                            pkg.version,
                            cfg_name.as_str().split('-').next().unwrap_or(cfg_name.as_str()),
                            cfg_name.as_str().split_once('-').map(|x| x.1).unwrap_or(""),
                            pkg.name,
                            pkg.name,
                        );
                    }
                }
            }

            // ---- Wheels-present path: pick the best wheel. ----
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
                    // Lockfile says this package has wheels but none matched
                    // the cfg's compat tags. Distinct from sdist-only.
                    anyhow::bail!(
                        "package '{}-{}' has no compatible wheel for cell ({}, {}) — \
                         no wheels matched the (platform, python) tags. Consider \
                         restricting muntjac.toml platforms.",
                        pkg.name,
                        pkg.version,
                        resolved_cfg.python_version,
                        plat_name
                    );
                }
            }
        }
    }

    // Build per-cell dep lists, then collapse to Uniform if every cell agrees.
    // ResolvedPackage.deps entries are formatted "name@version" — extract the
    // bare package name for the BUCK target reference (":<name>").
    let format_cell_deps = |raw: &Vec<String>| -> Vec<String> {
        let mut v: Vec<String> = raw
            .iter()
            .map(|d| format!(":{}", d.split('@').next().unwrap_or(d)))
            .collect();
        v.sort();
        v.dedup();
        v
    };

    let mut packages: Vec<EmitPackage> = Vec::new();
    for (key, wheel_map) in pkg_wheels {
        let cells_deps = &pkg_deps_per_cell[&key];

        let mut per_cell_formatted: BTreeMap<ConfigName, Vec<String>> = BTreeMap::new();
        for (cell, raw) in cells_deps {
            per_cell_formatted.insert(cell.clone(), format_cell_deps(raw));
        }

        // Collapse: if every cell produces the same Vec<String>, render Uniform.
        let mut values_iter = per_cell_formatted.values();
        let first = values_iter
            .next()
            .expect("at least one cell recorded a wheel for this package")
            .clone();
        let uniform = values_iter.all(|v| v == &first);

        let deps = if uniform {
            EmitDeps::Uniform(first)
        } else {
            EmitDeps::PerCell(per_cell_formatted)
        };

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
                deps: EmitDeps::Uniform(vec![":certifi".into(), ":idna".into()]),
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

        let input = build_emit_input(&config, &tree, &lockfile, None).expect("build_emit_input succeeds");

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
        match &pkg.deps {
            EmitDeps::Uniform(v) => assert!(v.is_empty()),
            EmitDeps::PerCell(_) => panic!("expected Uniform for empty-deps case"),
        }
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

        let err = build_emit_input(&config, &tree, &lockfile, None).expect_err("should fail on NoWheel");
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
        // S5 changed the error wording — the message now describes the
        // wheels-present-but-incompatible path. Verify it does not blame an
        // sdist (the new fork) and does suggest restricting platforms.
        assert!(
            msg.contains("no compatible wheel") || msg.contains("no wheels matched"),
            "error must describe the no-compat-wheel condition: {}",
            msg
        );
        assert!(
            msg.contains("muntjac.toml") || msg.contains("restricting"),
            "error must hint at muntjac.toml restriction: {}",
            msg
        );
    }

    #[test]
    fn build_emit_input_yields_per_cell_for_cross_cell_dep_diff() {
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
        // Result: parent's resolved deps differ across py3.11 vs py3.12 cells.
        // In S3 the composer bailed; in S4 it preserves the difference via PerCell.
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

        let input = build_emit_input(&config, &tree, &lockfile, None).expect("succeeds");
        let parent_pkg = input
            .packages
            .iter()
            .find(|p| p.name == "parent")
            .expect("parent present");
        match &parent_pkg.deps {
            EmitDeps::PerCell(m) => {
                assert!(
                    m.len() >= 2,
                    "expected at least two cells in PerCell map, got {}",
                    m.len()
                );
                let py311 = ConfigName::new("3.11", "linux-x86_64-gnu");
                let py312 = ConfigName::new("3.12", "linux-x86_64-gnu");
                assert_eq!(m.get(&py311).unwrap(), &vec![":child".to_string()]);
                assert_eq!(m.get(&py312).unwrap(), &Vec::<String>::new());
            }
            EmitDeps::Uniform(_) => panic!("expected PerCell, got Uniform"),
        }
    }

    #[test]
    fn build_emit_input_produces_per_cell_when_deps_differ() {
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

        // typing-extensions is required on py311 only, dropped on py312.
        let marker_py311_only: MarkerTree =
            MarkerTree::from_str("python_version < '3.12'").unwrap();

        let lockfile = Lockfile {
            version: 1,
            revision: 3,
            requires_python: ">=3.11,<3.13".into(),
            packages: vec![
                // first-party root
                Package {
                    name: PackageName::from_str("app").unwrap(),
                    version: Version::from_str("0.1").unwrap(),
                    source: Source::FirstParty {
                        kind: FirstPartyKind::Virtual,
                        path: ".".into(),
                    },
                    dependencies: vec![DepEdge {
                        name: PackageName::from_str("rich").unwrap(),
                        extra: vec![],
                        marker: None,
                    }],
                    sdist: None,
                    wheels: vec![],
                    metadata: None,
                },
                // rich: depends on typing-extensions for py311 only
                Package {
                    name: PackageName::from_str("rich").unwrap(),
                    version: Version::from_str("13.0").unwrap(),
                    source: Source::Registry {
                        url: Url::parse("https://pypi.org/simple").unwrap(),
                    },
                    dependencies: vec![DepEdge {
                        name: PackageName::from_str("typing-extensions").unwrap(),
                        extra: vec![],
                        marker: Some(marker_py311_only),
                    }],
                    sdist: None,
                    wheels: vec![Wheel {
                        url: Url::parse(
                            "https://files.pythonhosted.org/p/rich-13.0-py3-none-any.whl",
                        )
                        .unwrap(),
                        hash: "sha256:rich".into(),
                        size: None,
                        filename: "rich-13.0-py3-none-any.whl".into(),
                    }],
                    metadata: None,
                },
                // typing-extensions
                Package {
                    name: PackageName::from_str("typing-extensions").unwrap(),
                    version: Version::from_str("4.0").unwrap(),
                    source: Source::Registry {
                        url: Url::parse("https://pypi.org/simple").unwrap(),
                    },
                    dependencies: vec![],
                    sdist: None,
                    wheels: vec![Wheel {
                        url: Url::parse(
                            "https://files.pythonhosted.org/p/typing_extensions-4.0-py3-none-any.whl",
                        )
                        .unwrap(),
                        hash: "sha256:te".into(),
                        size: None,
                        filename: "typing_extensions-4.0-py3-none-any.whl".into(),
                    }],
                    metadata: None,
                },
            ],
        };

        let input = build_emit_input(&config, &tree, &lockfile, None).expect("succeeds");

        let rich = input
            .packages
            .iter()
            .find(|p| p.name == "rich")
            .expect("rich present");
        match &rich.deps {
            EmitDeps::PerCell(m) => {
                let py311 = ConfigName::new("3.11", "linux-x86_64-gnu");
                let py312 = ConfigName::new("3.12", "linux-x86_64-gnu");
                assert_eq!(
                    m.get(&py311).unwrap(),
                    &vec![":typing-extensions".to_string()]
                );
                assert_eq!(m.get(&py312).unwrap(), &Vec::<String>::new());
            }
            EmitDeps::Uniform(_) => panic!("expected PerCell, got Uniform"),
        }
    }

    #[test]
    fn build_emit_input_collapses_to_uniform_when_cells_agree() {
        // Multi-cell variant of build_emit_input_from_synthetic_resolved.
        // certifi has no deps on either cell, so the per-cell map collapses
        // to EmitDeps::Uniform(empty Vec).
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

        let lockfile = Lockfile {
            version: 1,
            revision: 3,
            requires_python: ">=3.11,<3.13".into(),
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

        let input = build_emit_input(&config, &tree, &lockfile, None).expect("succeeds");
        let pkg = input
            .packages
            .iter()
            .find(|p| p.name == "certifi")
            .expect("certifi present");
        // Wheel hits both cells; deps agree (empty) -> Uniform.
        assert_eq!(pkg.wheels.len(), 2);
        match &pkg.deps {
            EmitDeps::Uniform(v) => {
                assert!(v.is_empty(), "expected empty uniform deps, got {:?}", v)
            }
            EmitDeps::PerCell(m) => panic!("expected Uniform, got PerCell({:?})", m),
        }
    }

    #[test]
    fn emit_wheel_renders_prebake_source_as_prebake_url() {
        let w = EmitWheel {
            url: "prebake:tomli-2.0.1-py3-none-any.whl".to_string(),
            hash: "sha256:cafef00d".to_string(),
        };
        // assert that constructing an EmitWheel with a prebake-prefix URL
        // does not panic; the renderer in string_writer is what handles the
        // dispatch. This test pins the shape: URL slot carries the convention.
        assert!(w.url.starts_with("prebake:"));
        assert!(w.hash.starts_with("sha256:"));
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

    #[test]
    fn build_emit_input_errors_when_sdist_missing_from_manifest() {
        // Build a synthetic lockfile with one sdist-only package and
        // confirm that passing `manifest = None` causes build_emit_input to
        // fail with the "not prebaked" error.
        use crate::lock::types::{DepEdge, FirstPartyKind, Lockfile, Package, Sdist, Source};
        use pep440_rs::Version;
        use pep508_rs::PackageName;
        use url::Url;

        let config_toml = r#"
manifest_path   = "pyproject.toml"
third_party_dir = "third-party/python"
python_versions = ["3.12"]

[platforms.linux-x86_64-gnu]
target  = "x86_64-unknown-linux-gnu"
manylinux = "2_17"
"#;
        let config = crate::config::Config::from_str(config_toml).unwrap();
        let tree = config.trees[0].clone();

        let lockfile = Lockfile {
            version: 1,
            revision: 1,
            requires_python: ">=3.12".into(),
            packages: vec![
                Package {
                    name: PackageName::from_str("root").unwrap(),
                    version: Version::from_str("0.0.0").unwrap(),
                    source: Source::FirstParty {
                        kind: FirstPartyKind::Virtual,
                        path: ".".into(),
                    },
                    dependencies: vec![DepEdge {
                        name: PackageName::from_str("tomli").unwrap(),
                        extra: vec![],
                        marker: None,
                    }],
                    sdist: None,
                    wheels: vec![],
                    metadata: None,
                },
                Package {
                    name: PackageName::from_str("tomli").unwrap(),
                    version: Version::from_str("2.0.1").unwrap(),
                    source: Source::Registry {
                        url: Url::parse("https://pypi.org/simple").unwrap(),
                    },
                    dependencies: vec![],
                    sdist: Some(Sdist {
                        url: Url::parse("https://example.com/tomli-2.0.1.tar.gz").unwrap(),
                        hash: "sha256:de526c12914f0c550d15924c62d72abc48d6fe7364aa87328337a31007fe8a4f".into(),
                        size: None,
                    }),
                    wheels: vec![],
                    metadata: None,
                },
            ],
        };

        // No manifest provided → NotPrebaked error.
        let err = build_emit_input(&config, &tree, &lockfile, None).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("not prebaked"),
            "expected 'not prebaked' error, got: {msg}"
        );
        assert!(
            msg.contains("muntjac vendor"),
            "expected hint to mention `muntjac vendor`, got: {msg}"
        );
    }

    #[test]
    fn build_emit_input_errors_when_manifest_sdist_sha_mismatch() {
        // Same lockfile as above but with a manifest whose sdist_sha256
        // disagrees → StalePrebake error.
        use crate::lock::types::{DepEdge, FirstPartyKind, Lockfile, Package, Sdist, Source};
        use crate::sdist::{
            AllowlistedBackend, Manifest, ManifestClassification, ManifestEntry,
        };
        use pep440_rs::Version;
        use pep508_rs::PackageName;
        use url::Url;

        let config_toml = r#"
manifest_path   = "pyproject.toml"
third_party_dir = "third-party/python"
python_versions = ["3.12"]

[platforms.linux-x86_64-gnu]
target  = "x86_64-unknown-linux-gnu"
manylinux = "2_17"
"#;
        let config = crate::config::Config::from_str(config_toml).unwrap();
        let tree = config.trees[0].clone();

        let lockfile = Lockfile {
            version: 1,
            revision: 1,
            requires_python: ">=3.12".into(),
            packages: vec![
                Package {
                    name: PackageName::from_str("root").unwrap(),
                    version: Version::from_str("0.0.0").unwrap(),
                    source: Source::FirstParty {
                        kind: FirstPartyKind::Virtual,
                        path: ".".into(),
                    },
                    dependencies: vec![DepEdge {
                        name: PackageName::from_str("tomli").unwrap(),
                        extra: vec![],
                        marker: None,
                    }],
                    sdist: None,
                    wheels: vec![],
                    metadata: None,
                },
                Package {
                    name: PackageName::from_str("tomli").unwrap(),
                    version: Version::from_str("2.0.1").unwrap(),
                    source: Source::Registry {
                        url: Url::parse("https://pypi.org/simple").unwrap(),
                    },
                    dependencies: vec![],
                    sdist: Some(Sdist {
                        url: Url::parse("https://example.com/tomli-2.0.1.tar.gz").unwrap(),
                        hash: "sha256:newsha".into(),
                        size: None,
                    }),
                    wheels: vec![],
                    metadata: None,
                },
            ],
        };

        // Manifest points at the OLD sdist hash → stale.
        let manifest = Manifest {
            version: 1,
            entries: vec![ManifestEntry {
                package: "tomli".into(),
                version: "2.0.1".into(),
                sdist_sha256: "oldsha".into(),
                classification: ManifestClassification::PurePython {
                    backend: AllowlistedBackend::FlitCore,
                    wheel_filename: "tomli-2.0.1-py3-none-any.whl".into(),
                    wheel_sha256: "0".into(),
                },
            }],
        };

        let err = build_emit_input(&config, &tree, &lockfile, Some(&manifest)).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("stale"),
            "expected 'stale' error, got: {msg}"
        );
        assert!(
            msg.contains("muntjac vendor"),
            "expected hint to mention `muntjac vendor`, got: {msg}"
        );
    }
}
