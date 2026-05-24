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
    /// Overlay file list (NEW in S6). `None` = no overlay.
    pub overlay: Option<EmitOverlay>,
    /// Entry-point names to emit `python_binary` rules for (NEW in S6).
    /// Empty vec = no binaries.
    pub entry_points: Vec<String>,
    /// Top-level alias visibility (NEW in S6). `None` = `["PUBLIC"]`.
    pub visibility: Option<Vec<String>>,
    /// Top-level alias labels (NEW in S6).
    pub labels: Vec<String>,
    /// Env applied to emitted `python_binary` rules (NEW in S6).
    pub runtime_env: std::collections::BTreeMap<String, String>,
}

/// Overlay file list. Each entry is `(path_in_wheel, src_path_rel_to_tpd)`.
#[derive(Debug, Clone)]
pub struct EmitOverlay {
    pub files: Vec<(String, String)>,
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
    fixups: Option<&crate::fixup::FixupSet>,
    // Absolute path to the resolved `third_party_dir` for filesystem operations
    // (overlay walk). When `None`, falls back to `tree.third_party_dir` (works
    // when it's already absolute, e.g. in unit tests using `TempDir`).
    abs_third_party_dir: Option<&std::path::Path>,
) -> anyhow::Result<EmitInput> {
    use crate::fixup::cfg::split_target_triple;
    use crate::fixup::{CfgContext, resolve_for_cell};
    use pep440_rs::Version as PepVersion;

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
        let (arch, os, env) = split_target_triple(&plat.target);

        for pkg in &resolved_cfg.packages {
            let key: PkgKey = (pkg.name.clone(), pkg.version.clone());
            let wheels = wheel_index.get(&key).copied().unwrap_or(&[]);

            // Resolve fixup for (pkg, cell). None if no fixup configured.
            let resolved_fixup: Option<crate::fixup::ResolvedFixup> = fixups.and_then(|fs| {
                let name = pep508_rs::PackageName::from_str(&pkg.name).ok()?;
                let cfg = fs.get(&name)?;
                let pkg_ver = PepVersion::from_str(&pkg.version).ok()?;
                let py_ver = PepVersion::from_str(&resolved_cfg.python_version).ok()?;
                let ctx = CfgContext {
                    package_version: &pkg_ver,
                    python_version: py_ver,
                    target_os: &os,
                    target_arch: &arch,
                    target_env: &env,
                };
                Some(resolve_for_cell(cfg, &ctx))
            });

            // Apply fixup wheel-side ops (exclude_wheels filter + prefer_wheel override).
            let (wheels_owned, prefer_override): (Vec<Wheel>, Option<String>) = if let Some(rf) =
                &resolved_fixup
            {
                let pre_filter_count = wheels.len();
                let filtered = apply_exclude_wheels(wheels, &rf.exclude_wheels);
                let post_filter_count = filtered.len();
                if pre_filter_count > 0 && post_filter_count == 0 {
                    anyhow::bail!(
                        "exclude_wheels eliminates every wheel for {} on cell {}.\n  loosen the patterns or remove the fixup",
                        pkg.name,
                        cfg_name,
                    );
                }
                (filtered, rf.prefer_wheel.clone())
            } else {
                (wheels.to_vec(), None)
            };
            // Shadow the original &[Wheel] with the owned filtered slice.
            let wheels: &[Wheel] = &wheels_owned;

            // ---- Sdist-only path: consult the prebake manifest. ----
            if wheels.is_empty() {
                // Find the lockfile package to see whether there's an sdist.
                let lock_pkg = lockfile
                    .packages
                    .iter()
                    .find(|p| p.name.as_ref() == pkg.name && p.version.to_string() == pkg.version);
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
                        let mut cell_deps = pkg.deps.clone();
                        if let Some(rf) = &resolved_fixup {
                            apply_dep_ops(&mut cell_deps, rf);
                        }
                        pkg_deps_per_cell
                            .entry(key.clone())
                            .or_default()
                            .insert(cfg_name.clone(), cell_deps);
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
                            cfg_name
                                .as_str()
                                .split('-')
                                .next()
                                .unwrap_or(cfg_name.as_str()),
                            cfg_name.as_str().split_once('-').map(|x| x.1).unwrap_or(""),
                            pkg.name,
                            pkg.name,
                        );
                    }
                }
            }

            // ---- Wheels-present path: pick the best wheel. ----
            // prefer_wheel override: bypass picker and use a specific sha256.
            if let Some(sha) = &prefer_override {
                let want = sha.trim_start_matches("sha256:");
                if let Some(wheel) = wheels
                    .iter()
                    .find(|w| w.hash.trim_start_matches("sha256:") == want)
                {
                    pkg_wheels.entry(key.clone()).or_default().insert(
                        cfg_name.clone(),
                        EmitWheel {
                            url: wheel.url.to_string(),
                            hash: wheel.hash.clone(),
                        },
                    );
                    let mut cell_deps = pkg.deps.clone();
                    if let Some(rf) = &resolved_fixup {
                        apply_dep_ops(&mut cell_deps, rf);
                    }
                    pkg_deps_per_cell
                        .entry(key.clone())
                        .or_default()
                        .insert(cfg_name.clone(), cell_deps);
                    continue;
                }
                // prefer_wheel set but not found in filtered wheels.
                let available: Vec<String> = wheels
                    .iter()
                    .map(|w| w.hash.trim_start_matches("sha256:").to_string())
                    .collect();
                anyhow::bail!(
                    "prefer_wheel sha256:{} not found for {} on cell {}.\n  available wheel shas: {}",
                    want,
                    pkg.name,
                    cfg_name,
                    available.join(", "),
                );
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
                    let mut cell_deps = pkg.deps.clone();
                    if let Some(rf) = &resolved_fixup {
                        apply_dep_ops(&mut cell_deps, rf);
                    }
                    pkg_deps_per_cell
                        .entry(key.clone())
                        .or_default()
                        .insert(cfg_name.clone(), cell_deps);
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
            .map(|d| {
                if let Some(target) = d.strip_prefix("__BUCK_TARGET__") {
                    target.to_string()
                } else {
                    format!(":{}", d.split('@').next().unwrap_or(d))
                }
            })
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

        // S6: per-package fields (visibility/labels/entry_points/overlay/runtime_env)
        // are cell-independent at the top level. Resolve from the FIRST cell so
        // top-level body survives; cfg-section firing reflects ONE valid view.
        let first_cell_rf: Option<crate::fixup::ResolvedFixup> = if let Some(fs) = fixups {
            let pkg_name = pep508_rs::PackageName::from_str(&key.0).ok();
            pkg_name.and_then(|n| fs.get(&n)).map(|fc| {
                let first_cfg = &configs[0];
                let s_cfg = first_cfg.as_str();
                // "py312-linux-x86_64-gnu" -> ("py312", "linux-x86_64-gnu")
                let (py_str, plat_name) = match s_cfg.split_once('-') {
                    Some((p, r)) => (p.trim_start_matches("py"), r),
                    None => ("", ""),
                };
                // "312" -> "3.12"
                let py_str_dotted = if py_str.len() >= 2 {
                    format!("{}.{}", &py_str[0..1], &py_str[1..])
                } else {
                    py_str.to_string()
                };
                let plat = config.platforms.get(plat_name).expect("platform present");
                let (arch, os, env) = crate::fixup::cfg::split_target_triple(&plat.target);
                let pkg_ver = pep440_rs::Version::from_str(&key.1).expect("valid version");
                let py_ver =
                    pep440_rs::Version::from_str(&py_str_dotted).expect("valid python version");
                let ctx = crate::fixup::CfgContext {
                    package_version: &pkg_ver,
                    python_version: py_ver,
                    target_os: &os,
                    target_arch: &arch,
                    target_env: &env,
                };
                crate::fixup::resolve_for_cell(fc, &ctx)
            })
        } else {
            None
        };

        // entry_points: validate form. Auto(true) errors per spec §6.
        let entry_points_vec: Vec<String> = match &first_cell_rf {
            Some(rf) => match &rf.entry_points {
                Some(crate::fixup::EntryPoints::Auto(true)) => {
                    anyhow::bail!(
                        "entry_points = true is not supported in v1; list the binaries explicitly (e.g. entry_points = [\"ruff\"]).\n  package: {}",
                        key.0
                    );
                }
                Some(crate::fixup::EntryPoints::Auto(false)) | None => vec![],
                Some(crate::fixup::EntryPoints::Named(names)) => names.clone(),
            },
            None => vec![],
        };

        // Validate extra_deps / replace_deps Buck targets.
        if let Some(rf) = &first_cell_rf {
            for ed in &rf.extra_deps {
                if !crate::fixup::is_valid_buck_target(ed) {
                    anyhow::bail!(
                        "extra_deps target for {} is not a valid Buck target: `{}`\n  expected //path:name or :name form",
                        key.0,
                        ed
                    );
                }
            }
            for target in rf.replace_deps.values() {
                if !crate::fixup::is_valid_buck_target(target) {
                    anyhow::bail!(
                        "replace_deps target for {} is not a valid Buck target: `{}`\n  expected //path:name or :name form",
                        key.0,
                        target
                    );
                }
            }
        }

        // Overlay file discovery (filesystem walk via discover_overlay_files).
        let overlay_emit: Option<EmitOverlay> = if let Some(rf) = &first_cell_rf {
            if let Some(overlay_rel) = &rf.overlay {
                // Use the absolute path for FS operations when provided; fall
                // back to tree.third_party_dir for unit tests that already
                // supply an absolute TempDir path.
                let tpd_for_fs = abs_third_party_dir.unwrap_or(&tree.third_party_dir);
                let fixup_dir = tpd_for_fs.join("fixups").join(&key.0);
                let files = crate::fixup::discover_overlay_files(
                    &key.0,
                    tpd_for_fs,
                    &fixup_dir,
                    overlay_rel,
                )?;
                Some(EmitOverlay { files })
            } else {
                None
            }
        } else {
            None
        };

        packages.push(EmitPackage {
            name: key.0.clone(),
            version: key.1.clone(),
            deps,
            wheels: wheel_map,
            overlay: overlay_emit,
            entry_points: entry_points_vec,
            visibility: first_cell_rf.as_ref().and_then(|r| r.visibility.clone()),
            labels: first_cell_rf
                .as_ref()
                .map(|r| r.labels.clone())
                .unwrap_or_default(),
            runtime_env: first_cell_rf
                .as_ref()
                .map(|r| r.runtime_env.clone())
                .unwrap_or_default(),
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

/// Apply a `ResolvedFixup`'s dep-side ops to a package's raw dep list.
/// Raw deps are formatted as "name@version" by the lock-resolver.
///
/// Order:
///   1. `replace_deps`: substitute package names. The "name" portion of "name@version" is matched.
///   2. `omit_deps`: drop entries matching by name.
///   3. `extra_deps`: append (verbatim Buck targets — no transformation).
fn apply_dep_ops(deps: &mut Vec<String>, fixup: &crate::fixup::ResolvedFixup) {
    let extract_name = |s: &str| s.split('@').next().unwrap_or(s).to_string();

    // 1. replace: rewrite each entry whose name is in replace_deps.
    for dep in deps.iter_mut() {
        let name = extract_name(dep);
        if let Some(target) = fixup.replace_deps.get(&name) {
            // The replacement is a full Buck target; mark it with a sentinel
            // so format_cell_deps knows NOT to add the ":" prefix later.
            *dep = format!("__BUCK_TARGET__{}", target);
        }
    }

    // 2. omit: drop by name (does not affect already-replaced sentinels).
    deps.retain(|d| {
        if d.starts_with("__BUCK_TARGET__") {
            return true;
        }
        let name = extract_name(d);
        !fixup.omit_deps.iter().any(|o| o == &name)
    });

    // 3. extra: append as Buck-target sentinels.
    for ed in &fixup.extra_deps {
        deps.push(format!("__BUCK_TARGET__{}", ed));
    }
}

fn apply_exclude_wheels(wheels: &[Wheel], patterns: &[String]) -> Vec<Wheel> {
    if patterns.is_empty() {
        return wheels.to_vec();
    }
    let patterns: Vec<glob::Pattern> = patterns
        .iter()
        .filter_map(|p| glob::Pattern::new(p).ok())
        .collect();
    wheels
        .iter()
        .filter(|w| !patterns.iter().any(|p| p.matches(&w.filename)))
        .cloned()
        .collect()
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
                overlay: None,
                entry_points: vec![],
                visibility: None,
                labels: vec![],
                runtime_env: BTreeMap::new(),
            }],
        };
        assert_eq!(inp.tree, "default");
        assert_eq!(inp.packages.len(), 1);
        assert_eq!(inp.configs[0].as_str(), "py312-linux-x86_64-gnu");
    }

    #[test]
    fn emit_package_with_fixup_fields_constructs() {
        use std::collections::BTreeMap;

        let mut rt_env = BTreeMap::new();
        rt_env.insert("LIBJPEG_PATH".into(), "/opt/libjpeg/lib".into());

        let pkg = EmitPackage {
            name: "pillow".into(),
            version: "10.0.0".into(),
            deps: EmitDeps::Uniform(vec![]),
            wheels: BTreeMap::new(),
            overlay: Some(EmitOverlay {
                files: vec![(
                    "PIL/_imaging.py".into(),
                    "fixups/pillow/overlay/PIL/_imaging.py".into(),
                )],
            }),
            entry_points: vec!["pillow-cli".into()],
            visibility: Some(vec!["//apps/imaging/...".into()]),
            labels: vec!["security-sensitive".into()],
            runtime_env: rt_env,
        };
        assert_eq!(pkg.entry_points, vec!["pillow-cli"]);
        assert_eq!(pkg.overlay.unwrap().files.len(), 1);
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

        let input = build_emit_input(&config, &tree, &lockfile, None, None, None)
            .expect("build_emit_input succeeds");

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

        let err = build_emit_input(&config, &tree, &lockfile, None, None, None)
            .expect_err("should fail on NoWheel");
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

        let input =
            build_emit_input(&config, &tree, &lockfile, None, None, None).expect("succeeds");
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

        let input =
            build_emit_input(&config, &tree, &lockfile, None, None, None).expect("succeeds");

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

        let input =
            build_emit_input(&config, &tree, &lockfile, None, None, None).expect("succeeds");
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
        let err = build_emit_input(&config, &tree, &lockfile, None, None, None).unwrap_err();
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
        use crate::sdist::{AllowlistedBackend, Manifest, ManifestClassification, ManifestEntry};
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

        let err =
            build_emit_input(&config, &tree, &lockfile, Some(&manifest), None, None).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("stale"), "expected 'stale' error, got: {msg}");
        assert!(
            msg.contains("muntjac vendor"),
            "expected hint to mention `muntjac vendor`, got: {msg}"
        );
    }

    #[test]
    fn fixup_extra_deps_appends() {
        use crate::config::{Config, Platform, PythonVersion, Tree};
        use crate::fixup::FixupSet;
        use crate::fixup::schema::{FixupBody, FixupConfig};
        use crate::lock::types::{DepEdge, FirstPartyKind, Lockfile, Package, Source, Wheel};
        use pep440_rs::Version;
        use pep508_rs::PackageName;
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
                        url: Url::parse("https://files.pythonhosted.org/p/certifi.whl").unwrap(),
                        hash: "sha256:abc".into(),
                        size: None,
                        filename: "certifi-2025.4.26-py3-none-any.whl".into(),
                    }],
                    metadata: None,
                },
            ],
        };

        let mut fixups_map = std::collections::BTreeMap::new();
        fixups_map.insert(
            PackageName::from_str("certifi").unwrap(),
            FixupConfig {
                top: FixupBody {
                    extra_deps: vec!["//third-party/c:openssl".into()],
                    ..Default::default()
                },
                cfg_sections: vec![],
                replace_community: false,
            },
        );
        let fixups = FixupSet::from_map_for_test(fixups_map);

        let input = build_emit_input(&config, &tree, &lockfile, None, Some(&fixups), None)
            .expect("build_emit_input with fixups");
        let pkg = &input.packages[0];
        assert_eq!(pkg.name, "certifi");
        match &pkg.deps {
            EmitDeps::Uniform(deps) => {
                assert!(
                    deps.contains(&"//third-party/c:openssl".to_string()),
                    "deps did not contain the extra_dep: {:?}",
                    deps
                );
            }
            other => panic!("expected Uniform, got {:?}", other),
        }
    }

    #[test]
    fn fixup_prefer_wheel_overrides_picker() {
        use crate::config::{Config, Platform, PythonVersion, Tree};
        use crate::fixup::FixupSet;
        use crate::fixup::schema::{FixupBody, FixupConfig};
        use crate::lock::types::{DepEdge, FirstPartyKind, Lockfile, Package, Source, Wheel};
        use pep440_rs::Version;
        use pep508_rs::PackageName;
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
        // Two wheels; one would be the picker's natural choice.
        // prefer_wheel = sha256:xyz selects the OTHER one.
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
                    wheels: vec![
                        Wheel {
                            url: Url::parse("https://files.pythonhosted.org/p/certifi-v1.whl")
                                .unwrap(),
                            hash: "sha256:abc".into(),
                            size: None,
                            filename: "certifi-2025.4.26-py3-none-any.whl".into(),
                        },
                        Wheel {
                            url: Url::parse("https://files.pythonhosted.org/p/certifi-v2.whl")
                                .unwrap(),
                            hash: "sha256:xyz".into(),
                            size: None,
                            filename: "certifi-2025.4.26-py3-none-any.whl".into(),
                        },
                    ],
                    metadata: None,
                },
            ],
        };

        let mut fixups_map = std::collections::BTreeMap::new();
        fixups_map.insert(
            PackageName::from_str("certifi").unwrap(),
            FixupConfig {
                top: FixupBody {
                    prefer_wheel: Some("sha256:xyz".into()),
                    ..Default::default()
                },
                cfg_sections: vec![],
                replace_community: false,
            },
        );
        let fixups = FixupSet::from_map_for_test(fixups_map);

        let input = build_emit_input(&config, &tree, &lockfile, None, Some(&fixups), None)
            .expect("build_emit_input with prefer_wheel");
        let pkg = &input.packages[0];
        let wheel = pkg.wheels.values().next().unwrap();
        assert_eq!(
            wheel.hash, "sha256:xyz",
            "prefer_wheel should override picker"
        );
    }

    /// Shared harness: app → certifi 2025.4.26 with two wheels (sha256:abc,
    /// sha256:xyz). Single cell linux-x86_64-gnu @ py3.12. The lock graph is
    /// the minimum needed to drive a single resolved package through the
    /// emitter. Used by the prefer_wheel + exclude_wheels tests below.
    #[allow(clippy::type_complexity)]
    fn two_wheel_certifi_harness() -> (
        crate::config::Tree,
        crate::config::Config,
        crate::lock::types::Lockfile,
    ) {
        use crate::config::{Config, Platform, PythonVersion, Tree};
        use crate::lock::types::{DepEdge, FirstPartyKind, Lockfile, Package, Source, Wheel};
        use pep440_rs::Version;
        use pep508_rs::PackageName;
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
                    wheels: vec![
                        Wheel {
                            url: Url::parse("https://files.pythonhosted.org/p/certifi-v1.whl")
                                .unwrap(),
                            hash: "sha256:abc".into(),
                            size: None,
                            filename: "certifi-2025.4.26-py3-none-any.whl".into(),
                        },
                        Wheel {
                            url: Url::parse("https://files.pythonhosted.org/p/certifi-v2.whl")
                                .unwrap(),
                            hash: "sha256:xyz".into(),
                            size: None,
                            filename: "certifi-2025.4.26-py3-none-any.whl".into(),
                        },
                    ],
                    metadata: None,
                },
            ],
        };
        (tree, config, lockfile)
    }

    /// prefer_wheel pointing at an absent sha must bail with the canonical
    /// message listing the available shas (verbatim wording locked by spec §6).
    #[test]
    fn fixup_prefer_wheel_not_found_errors() {
        use crate::fixup::FixupSet;
        use crate::fixup::schema::{FixupBody, FixupConfig};
        use pep508_rs::PackageName;

        let (tree, config, lockfile) = two_wheel_certifi_harness();
        let mut fixups_map = std::collections::BTreeMap::new();
        fixups_map.insert(
            PackageName::from_str("certifi").unwrap(),
            FixupConfig {
                top: FixupBody {
                    prefer_wheel: Some("sha256:notfound".into()),
                    ..Default::default()
                },
                cfg_sections: vec![],
                replace_community: false,
            },
        );
        let fixups = FixupSet::from_map_for_test(fixups_map);

        let err = build_emit_input(&config, &tree, &lockfile, None, Some(&fixups), None)
            .expect_err("should fail when prefer_wheel sha is absent");
        let msg = format!("{:#}", err);
        assert!(
            msg.contains("prefer_wheel sha256:notfound not found for certifi"),
            "wrong message: {msg}"
        );
        assert!(
            msg.contains("available wheel shas:") && msg.contains("abc") && msg.contains("xyz"),
            "expected available shas in message: {msg}"
        );
    }

    /// exclude_wheels patterns that wipe out the entire wheel set must bail
    /// with the canonical message (verbatim wording locked by spec §6).
    #[test]
    fn fixup_exclude_wheels_leaves_none_errors() {
        use crate::fixup::FixupSet;
        use crate::fixup::schema::{FixupBody, FixupConfig};
        use pep508_rs::PackageName;

        let (tree, config, lockfile) = two_wheel_certifi_harness();
        let mut fixups_map = std::collections::BTreeMap::new();
        fixups_map.insert(
            PackageName::from_str("certifi").unwrap(),
            FixupConfig {
                top: FixupBody {
                    exclude_wheels: vec!["*.whl".into()],
                    ..Default::default()
                },
                cfg_sections: vec![],
                replace_community: false,
            },
        );
        let fixups = FixupSet::from_map_for_test(fixups_map);

        let err = build_emit_input(&config, &tree, &lockfile, None, Some(&fixups), None)
            .expect_err("should fail when exclude_wheels empties the wheel set");
        let msg = format!("{:#}", err);
        assert!(
            msg.contains("exclude_wheels eliminates every wheel for certifi"),
            "wrong message: {msg}"
        );
        assert!(
            msg.contains("loosen the patterns or remove the fixup"),
            "missing remediation hint: {msg}"
        );
    }

    /// Combining prefer_wheel with extra_deps proves the prefer-wheel happy
    /// path still threads through apply_dep_ops (the fixup_extra_deps_appends
    /// test covers the default picker path; this guards the second branch).
    #[test]
    fn fixup_prefer_wheel_still_applies_extra_deps() {
        use crate::fixup::FixupSet;
        use crate::fixup::schema::{FixupBody, FixupConfig};
        use pep508_rs::PackageName;

        let (tree, config, lockfile) = two_wheel_certifi_harness();
        let mut fixups_map = std::collections::BTreeMap::new();
        fixups_map.insert(
            PackageName::from_str("certifi").unwrap(),
            FixupConfig {
                top: FixupBody {
                    prefer_wheel: Some("sha256:xyz".into()),
                    extra_deps: vec!["//third-party/c:openssl".into()],
                    ..Default::default()
                },
                cfg_sections: vec![],
                replace_community: false,
            },
        );
        let fixups = FixupSet::from_map_for_test(fixups_map);

        let input = build_emit_input(&config, &tree, &lockfile, None, Some(&fixups), None)
            .expect("build_emit_input with prefer_wheel + extra_deps");
        let pkg = &input.packages[0];
        let wheel = pkg.wheels.values().next().unwrap();
        assert_eq!(wheel.hash, "sha256:xyz");
        match &pkg.deps {
            EmitDeps::Uniform(deps) => assert!(
                deps.contains(&"//third-party/c:openssl".to_string()),
                "prefer-wheel branch dropped extra_deps: {deps:?}"
            ),
            other => panic!("expected Uniform deps, got {other:?}"),
        }
    }

    #[test]
    fn fixup_visibility_and_entry_points_thread_to_emit_package() {
        use crate::fixup::FixupSet;
        use crate::fixup::schema::{EntryPoints, FixupBody, FixupConfig};
        use pep508_rs::PackageName;

        let (tree, config, lockfile) = two_wheel_certifi_harness();
        let mut fixups_map = std::collections::BTreeMap::new();
        let mut runtime_env = std::collections::BTreeMap::new();
        runtime_env.insert("FOO".into(), "bar".into());
        fixups_map.insert(
            PackageName::from_str("certifi").unwrap(),
            FixupConfig {
                top: FixupBody {
                    visibility: Some(vec!["//apps/imaging/...".into()]),
                    labels: vec!["security-sensitive".into()],
                    entry_points: Some(EntryPoints::Named(vec!["certifi-cli".into()])),
                    runtime_env,
                    // prefer_wheel keeps the test deterministic — picks one
                    // of the two wheels in the harness.
                    prefer_wheel: Some("sha256:abc".into()),
                    ..Default::default()
                },
                cfg_sections: vec![],
                replace_community: false,
            },
        );
        let fixups = FixupSet::from_map_for_test(fixups_map);

        let input = build_emit_input(&config, &tree, &lockfile, None, Some(&fixups), None)
            .expect("build_emit_input with per-package fixup fields");
        let pkg = &input.packages[0];
        assert_eq!(
            pkg.visibility.as_deref(),
            Some(&["//apps/imaging/...".to_string()][..])
        );
        assert_eq!(pkg.labels, vec!["security-sensitive"]);
        assert_eq!(pkg.entry_points, vec!["certifi-cli"]);
        assert_eq!(pkg.runtime_env.get("FOO").map(|s| s.as_str()), Some("bar"));
    }

    #[test]
    fn fixup_entry_points_auto_errors() {
        use crate::fixup::FixupSet;
        use crate::fixup::schema::{EntryPoints, FixupBody, FixupConfig};
        use pep508_rs::PackageName;

        let (tree, config, lockfile) = two_wheel_certifi_harness();
        let mut fixups_map = std::collections::BTreeMap::new();
        fixups_map.insert(
            PackageName::from_str("certifi").unwrap(),
            FixupConfig {
                top: FixupBody {
                    entry_points: Some(EntryPoints::Auto(true)),
                    prefer_wheel: Some("sha256:abc".into()),
                    ..Default::default()
                },
                cfg_sections: vec![],
                replace_community: false,
            },
        );
        let fixups = FixupSet::from_map_for_test(fixups_map);

        let err = build_emit_input(&config, &tree, &lockfile, None, Some(&fixups), None)
            .expect_err("entry_points = true must bail");
        let msg = format!("{:#}", err);
        assert!(
            msg.contains("entry_points = true is not supported in v1"),
            "wrong message: {msg}"
        );
        assert!(msg.contains("certifi"), "should name the package: {msg}");
    }

    #[test]
    fn fixup_extra_dep_invalid_buck_target_errors() {
        use crate::fixup::FixupSet;
        use crate::fixup::schema::{FixupBody, FixupConfig};
        use pep508_rs::PackageName;

        let (tree, config, lockfile) = two_wheel_certifi_harness();
        let mut fixups_map = std::collections::BTreeMap::new();
        fixups_map.insert(
            PackageName::from_str("certifi").unwrap(),
            FixupConfig {
                top: FixupBody {
                    extra_deps: vec!["not-a-target".into()], // missing :, missing //
                    prefer_wheel: Some("sha256:abc".into()),
                    ..Default::default()
                },
                cfg_sections: vec![],
                replace_community: false,
            },
        );
        let fixups = FixupSet::from_map_for_test(fixups_map);

        let err = build_emit_input(&config, &tree, &lockfile, None, Some(&fixups), None)
            .expect_err("invalid extra_deps target must bail");
        let msg = format!("{:#}", err);
        assert!(
            msg.contains("extra_deps target for certifi is not a valid Buck target"),
            "wrong message: {msg}"
        );
        assert!(
            msg.contains("not-a-target"),
            "should quote the bad target: {msg}"
        );
    }

    #[test]
    fn fixup_overlay_populates_emit_package_overlay() {
        use crate::fixup::FixupSet;
        use crate::fixup::schema::{FixupBody, FixupConfig};
        use pep508_rs::PackageName;
        use std::fs;
        use tempfile::TempDir;

        // Build a tree whose third_party_dir is a REAL filesystem path with
        // an overlay layout. We need this because discover_overlay_files
        // walks the actual disk.
        let tmp = TempDir::new().unwrap();
        let tpd = tmp.path().to_path_buf();
        let fixup_dir = tpd.join("fixups/certifi");
        fs::create_dir_all(fixup_dir.join("overlay/certifi")).unwrap();
        fs::write(
            fixup_dir.join("overlay/certifi/_paths.py"),
            "OVERRIDDEN = True\n",
        )
        .unwrap();

        // Build a one-cell harness pointing at the tempdir.
        use crate::config::{Config, Platform, PythonVersion, Tree};
        use crate::lock::types::{DepEdge, FirstPartyKind, Lockfile, Package, Source, Wheel};
        use pep440_rs::Version;
        use url::Url;

        let tree = Tree {
            name: "default".into(),
            manifest_path: "pyproject.toml".into(),
            third_party_dir: tpd.clone(),
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
                        url: Url::parse("https://files.pythonhosted.org/p/certifi.whl").unwrap(),
                        hash: "sha256:abc".into(),
                        size: None,
                        filename: "certifi-2025.4.26-py3-none-any.whl".into(),
                    }],
                    metadata: None,
                },
            ],
        };

        let mut fixups_map = std::collections::BTreeMap::new();
        fixups_map.insert(
            PackageName::from_str("certifi").unwrap(),
            FixupConfig {
                top: FixupBody {
                    overlay: Some("overlay".into()),
                    ..Default::default()
                },
                cfg_sections: vec![],
                replace_community: false,
            },
        );
        let fixups = FixupSet::from_map_for_test(fixups_map);

        let input = build_emit_input(&config, &tree, &lockfile, None, Some(&fixups), None)
            .expect("build_emit_input with overlay");
        let pkg = &input.packages[0];
        let overlay = pkg.overlay.as_ref().expect("overlay should be populated");
        assert_eq!(overlay.files.len(), 1);
        let (in_wheel, src_rel) = &overlay.files[0];
        assert_eq!(in_wheel, "certifi/_paths.py");
        assert!(
            src_rel.ends_with("fixups/certifi/overlay/certifi/_paths.py"),
            "src_rel should be tpd-relative: {src_rel}"
        );
    }

    #[test]
    fn build_emit_input_accepts_none_fixups() {
        // Goal: signature compiles + behavior is unchanged from no-fixup case.
        use crate::config::{Config, Platform, PythonVersion, Tree};
        use crate::lock::types::{DepEdge, FirstPartyKind, Lockfile, Package, Source, Wheel};
        use pep440_rs::Version;
        use pep508_rs::PackageName;
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
                        url: Url::parse("https://files.pythonhosted.org/p/certifi.whl").unwrap(),
                        hash: "sha256:abc".into(),
                        size: None,
                        filename: "certifi-2025.4.26-py3-none-any.whl".into(),
                    }],
                    metadata: None,
                },
            ],
        };

        let input = build_emit_input(&config, &tree, &lockfile, None, None, None)
            .expect("build_emit_input with None fixups");
        assert_eq!(input.packages.len(), 1);
    }
}
