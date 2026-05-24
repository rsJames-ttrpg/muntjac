//! Merge multiple `FixupBody` instances into a single `ResolvedFixup`.
//!
//! S6 is local-only: `resolve_for_cell` runs `merge_into` over the
//! top-level body, then over each cfg section whose predicate matches.
//! S7 will add the community layer as an outer wrapper.

use std::collections::BTreeMap;
use std::path::PathBuf;

use pep508_rs::PackageName;

use crate::fixup::cfg::{CfgContext, CfgPredicate};
use crate::fixup::loader::FixupSet;
use crate::fixup::schema::{EntryPoints, FixupBody, FixupConfig};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ResolvedFixup {
    pub extra_deps: Vec<String>,
    pub omit_deps: Vec<String>,
    pub replace_deps: BTreeMap<String, String>,
    pub prefer_wheel: Option<String>,
    pub exclude_wheels: Vec<String>,
    pub overlay: Option<PathBuf>,
    pub entry_points: Option<EntryPoints>,
    pub visibility: Option<Vec<String>>,
    pub labels: Vec<String>,
    pub runtime_env: BTreeMap<String, String>,
}

/// Merge `rhs` into `lhs`. List-valued fields accumulate (no dedupe;
/// emitter dedupes after applying omit_deps). Scalar fields replace
/// when rhs is `Some`. Map fields extend (later keys win).
pub fn merge_into(lhs: &mut ResolvedFixup, rhs: &FixupBody) {
    lhs.extra_deps.extend(rhs.extra_deps.iter().cloned());
    lhs.omit_deps.extend(rhs.omit_deps.iter().cloned());
    lhs.replace_deps
        .extend(rhs.replace_deps.iter().map(|(k, v)| (k.clone(), v.clone())));
    if rhs.prefer_wheel.is_some() {
        lhs.prefer_wheel = rhs.prefer_wheel.clone();
    }
    lhs.exclude_wheels
        .extend(rhs.exclude_wheels.iter().cloned());
    if rhs.overlay.is_some() {
        lhs.overlay = rhs.overlay.clone();
    }
    if rhs.entry_points.is_some() {
        lhs.entry_points = rhs.entry_points.clone();
    }
    if rhs.visibility.is_some() {
        lhs.visibility = rhs.visibility.clone();
    }
    lhs.labels.extend(rhs.labels.iter().cloned());
    lhs.runtime_env
        .extend(rhs.runtime_env.iter().map(|(k, v)| (k.clone(), v.clone())));
}

/// Cross-layer merge. Community is contributed first; local overrides
/// scalar/Option fields and extends list/map fields.
///
/// Rules:
///   - Vec<String>: community ++ local, dedup preserved-first
///   - BTreeMap<String, String>: extend (local key overwrites community)
///   - Option<T>: local wins if Some, else community
pub fn merge_resolved(community: ResolvedFixup, local: ResolvedFixup) -> ResolvedFixup {
    ResolvedFixup {
        extra_deps: concat_dedup(community.extra_deps, local.extra_deps),
        omit_deps: concat_dedup(community.omit_deps, local.omit_deps),
        replace_deps: extend_map(community.replace_deps, local.replace_deps),
        prefer_wheel: local.prefer_wheel.or(community.prefer_wheel),
        exclude_wheels: concat_dedup(community.exclude_wheels, local.exclude_wheels),
        overlay: local.overlay.or(community.overlay),
        entry_points: local.entry_points.or(community.entry_points),
        visibility: local.visibility.or(community.visibility),
        labels: concat_dedup(community.labels, local.labels),
        runtime_env: extend_map(community.runtime_env, local.runtime_env),
    }
}

/// Concatenate two Vec<String>, dedup preserving first occurrence.
fn concat_dedup(a: Vec<String>, b: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::with_capacity(a.len() + b.len());
    for s in a.into_iter().chain(b) {
        if seen.insert(s.clone()) {
            out.push(s);
        }
    }
    out
}

fn extend_map(
    mut a: BTreeMap<String, String>,
    b: BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    a.extend(b);
    a
}

/// Resolve a package's fixup for one cell. Returns the merged `ResolvedFixup`.
/// Predicate parse failures cause that one section to be silently skipped
/// (parse errors are reported at load time, not here).
pub fn resolve_for_cell(config: &FixupConfig, ctx: &CfgContext<'_>) -> ResolvedFixup {
    let mut out = ResolvedFixup::default();
    merge_into(&mut out, &config.top);
    for (predicate_str, body) in &config.cfg_sections {
        let Ok(predicate) = CfgPredicate::parse(predicate_str) else {
            continue; // parse error reported at load time, not here
        };
        if predicate.evaluate(ctx) {
            merge_into(&mut out, body);
        }
    }
    out
}

/// Two-layer fixup facade. Owns the community and local `FixupSet`s
/// and exposes a single `.resolve(pkg, ctx)` entry point that hides
/// the layering from the emitter.
#[derive(Debug, Default, Clone)]
pub struct EffectiveFixups {
    pub community: FixupSet,
    pub local: FixupSet,
}

impl EffectiveFixups {
    /// Load both layers per the registry configuration and the
    /// `allow_local_overrides` flag.
    ///
    /// - `RegistryConfig::None`        → community is empty.
    /// - `RegistryConfig::FileUrl(p)`  → community loaded from `<p>/packages/`.
    /// - `RegistryConfig::Git { .. }`  → errors with `GitRegistryNotImplemented` (S7b implements).
    /// - `allow_local_overrides=false` → local is empty regardless of disk state.
    pub fn load(
        registry: &crate::fixup::RegistryConfig,
        third_party_dir: &std::path::Path,
        allow_local_overrides: bool,
    ) -> Result<Self, crate::fixup::FixupError> {
        let community = match registry {
            crate::fixup::RegistryConfig::None => FixupSet::default(),
            crate::fixup::RegistryConfig::FileUrl(registry_dir) => {
                crate::fixup::load_community(registry_dir)?
            }
            crate::fixup::RegistryConfig::Git { url, .. } => {
                return Err(crate::fixup::FixupError::GitRegistryNotImplemented {
                    registry: url.clone(),
                });
            }
        };

        let local = if allow_local_overrides {
            crate::fixup::load_local(third_party_dir)?
        } else {
            FixupSet::default()
        };

        Ok(Self { community, local })
    }

    /// Resolve a package's fixup for one cell. Always returns a
    /// `ResolvedFixup` — default if neither layer has the package.
    ///
    /// Algorithm (per design spec §4.1):
    ///   1. Look up local first; if it has `replace_community = true`,
    ///      drop the community layer for this package.
    ///   2. Fully resolve each layer via `resolve_for_cell`.
    ///   3. Cross-layer merge via `merge_resolved`.
    pub fn resolve(&self, pkg: &PackageName, ctx: &CfgContext<'_>) -> ResolvedFixup {
        let local_cfg = self.local.get(pkg);
        let community_cfg = match local_cfg {
            Some(c) if c.replace_community => None,
            _ => self.community.get(pkg),
        };

        let community_resolved = community_cfg
            .map(|cfg| resolve_for_cell(cfg, ctx))
            .unwrap_or_default();
        let local_resolved = local_cfg
            .map(|cfg| resolve_for_cell(cfg, ctx))
            .unwrap_or_default();

        merge_resolved(community_resolved, local_resolved)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixup::cfg::CfgContext;
    use pep440_rs::Version;
    use std::str::FromStr;

    fn empty_ctx() -> (Version, Version) {
        (
            Version::from_str("1.0").unwrap(),
            Version::from_str("3.12").unwrap(),
        )
    }

    #[test]
    fn merge_lists_accumulate() {
        let mut acc = ResolvedFixup::default();
        let a = FixupBody {
            extra_deps: vec!["//a:b".into()],
            ..Default::default()
        };
        let b = FixupBody {
            extra_deps: vec!["//c:d".into()],
            ..Default::default()
        };
        merge_into(&mut acc, &a);
        merge_into(&mut acc, &b);
        assert_eq!(acc.extra_deps, vec!["//a:b", "//c:d"]);
    }

    #[test]
    fn merge_scalars_replace_when_some() {
        let mut acc = ResolvedFixup::default();
        let a = FixupBody {
            overlay: Some("first".into()),
            ..Default::default()
        };
        let b = FixupBody {
            overlay: Some("second".into()),
            ..Default::default()
        };
        let c = FixupBody {
            overlay: None,
            ..Default::default()
        };
        merge_into(&mut acc, &a);
        merge_into(&mut acc, &b);
        merge_into(&mut acc, &c);
        assert_eq!(acc.overlay.as_deref(), Some(std::path::Path::new("second")));
    }

    #[test]
    fn merge_maps_extend() {
        let mut acc = ResolvedFixup::default();
        let mut a_rd = BTreeMap::new();
        a_rd.insert("numpy".into(), "//company/numpy:numpy".into());
        let mut b_rd = BTreeMap::new();
        b_rd.insert("numpy".into(), "//new/numpy:numpy".into());
        b_rd.insert("scipy".into(), "//company/scipy:scipy".into());

        merge_into(
            &mut acc,
            &FixupBody {
                replace_deps: a_rd,
                ..Default::default()
            },
        );
        merge_into(
            &mut acc,
            &FixupBody {
                replace_deps: b_rd,
                ..Default::default()
            },
        );

        assert_eq!(
            acc.replace_deps.get("numpy").map(|s| s.as_str()),
            Some("//new/numpy:numpy")
        );
        assert_eq!(
            acc.replace_deps.get("scipy").map(|s| s.as_str()),
            Some("//company/scipy:scipy")
        );
    }

    #[test]
    fn resolve_picks_matching_cfg_sections() {
        let (v, py) = empty_ctx();
        let ctx = CfgContext {
            package_version: &v,
            python_version: py,
            target_os: "linux",
            target_arch: "x86_64",
            target_env: "gnu",
        };
        let config = FixupConfig {
            top: FixupBody {
                extra_deps: vec!["//base:dep".into()],
                ..Default::default()
            },
            cfg_sections: vec![
                (
                    "target_os = \"linux\"".into(),
                    FixupBody {
                        extra_deps: vec!["//linux:dep".into()],
                        ..Default::default()
                    },
                ),
                (
                    "target_os = \"macos\"".into(),
                    FixupBody {
                        extra_deps: vec!["//macos:dep".into()],
                        ..Default::default()
                    },
                ),
            ],
            replace_community: false,
        };
        let resolved = resolve_for_cell(&config, &ctx);
        assert_eq!(resolved.extra_deps, vec!["//base:dep", "//linux:dep"]);
    }

    #[test]
    fn resolve_with_no_matches_yields_top_only() {
        let (v, py) = empty_ctx();
        let ctx = CfgContext {
            package_version: &v,
            python_version: py,
            target_os: "macos",
            target_arch: "aarch64",
            target_env: "",
        };
        let config = FixupConfig {
            top: FixupBody {
                extra_deps: vec!["//base:dep".into()],
                ..Default::default()
            },
            cfg_sections: vec![(
                "target_os = \"linux\"".into(),
                FixupBody {
                    extra_deps: vec!["//linux:dep".into()],
                    ..Default::default()
                },
            )],
            replace_community: false,
        };
        let resolved = resolve_for_cell(&config, &ctx);
        assert_eq!(resolved.extra_deps, vec!["//base:dep"]);
    }

    #[test]
    fn merge_resolved_extra_deps_community_first_dedup() {
        let c = ResolvedFixup {
            extra_deps: vec!["//c:base".into(), "//shared:dep".into()],
            ..Default::default()
        };
        let l = ResolvedFixup {
            extra_deps: vec!["//shared:dep".into(), "//l:base".into()],
            ..Default::default()
        };
        let merged = super::merge_resolved(c, l);
        assert_eq!(
            merged.extra_deps,
            vec!["//c:base", "//shared:dep", "//l:base"]
        );
    }

    #[test]
    fn merge_resolved_omit_deps_community_first_dedup() {
        let c = ResolvedFixup {
            omit_deps: vec!["one".into(), "two".into()],
            ..Default::default()
        };
        let l = ResolvedFixup {
            omit_deps: vec!["two".into(), "three".into()],
            ..Default::default()
        };
        let merged = super::merge_resolved(c, l);
        assert_eq!(merged.omit_deps, vec!["one", "two", "three"]);
    }

    #[test]
    fn merge_resolved_replace_deps_local_wins_on_key_collision() {
        let mut c_rd = std::collections::BTreeMap::new();
        c_rd.insert("foo".to_string(), "//c:foo".to_string());
        c_rd.insert("only-community".to_string(), "//c:only".to_string());
        let c = ResolvedFixup {
            replace_deps: c_rd,
            ..Default::default()
        };
        let mut l_rd = std::collections::BTreeMap::new();
        l_rd.insert("foo".to_string(), "//l:foo".to_string());
        l_rd.insert("only-local".to_string(), "//l:only".to_string());
        let l = ResolvedFixup {
            replace_deps: l_rd,
            ..Default::default()
        };
        let merged = super::merge_resolved(c, l);
        assert_eq!(
            merged.replace_deps.get("foo").map(|s| s.as_str()),
            Some("//l:foo")
        );
        assert_eq!(
            merged
                .replace_deps
                .get("only-community")
                .map(|s| s.as_str()),
            Some("//c:only")
        );
        assert_eq!(
            merged.replace_deps.get("only-local").map(|s| s.as_str()),
            Some("//l:only")
        );
    }

    #[test]
    fn merge_resolved_prefer_wheel_local_some_wins() {
        let c = ResolvedFixup {
            prefer_wheel: Some("sha256:aaa".into()),
            ..Default::default()
        };
        let l = ResolvedFixup {
            prefer_wheel: Some("sha256:bbb".into()),
            ..Default::default()
        };
        let merged = super::merge_resolved(c, l);
        assert_eq!(merged.prefer_wheel.as_deref(), Some("sha256:bbb"));
    }

    #[test]
    fn merge_resolved_prefer_wheel_community_kept_when_local_none() {
        let c = ResolvedFixup {
            prefer_wheel: Some("sha256:aaa".into()),
            ..Default::default()
        };
        let l = ResolvedFixup::default();
        let merged = super::merge_resolved(c, l);
        assert_eq!(merged.prefer_wheel.as_deref(), Some("sha256:aaa"));
    }

    #[test]
    fn merge_resolved_exclude_wheels_concat_dedup() {
        let c = ResolvedFixup {
            exclude_wheels: vec!["*win32*".into()],
            ..Default::default()
        };
        let l = ResolvedFixup {
            exclude_wheels: vec!["*win32*".into(), "*cuda*".into()],
            ..Default::default()
        };
        let merged = super::merge_resolved(c, l);
        assert_eq!(merged.exclude_wheels, vec!["*win32*", "*cuda*"]);
    }

    #[test]
    fn merge_resolved_overlay_local_some_wins() {
        let c = ResolvedFixup {
            overlay: Some(std::path::PathBuf::from("c-overlay")),
            ..Default::default()
        };
        let l = ResolvedFixup {
            overlay: Some(std::path::PathBuf::from("l-overlay")),
            ..Default::default()
        };
        let merged = super::merge_resolved(c, l);
        assert_eq!(
            merged.overlay.as_deref(),
            Some(std::path::Path::new("l-overlay"))
        );
    }

    #[test]
    fn merge_resolved_entry_points_local_some_wins() {
        let c = ResolvedFixup {
            entry_points: Some(crate::fixup::EntryPoints::Named(vec!["c-bin".into()])),
            ..Default::default()
        };
        let l = ResolvedFixup {
            entry_points: Some(crate::fixup::EntryPoints::Named(vec!["l-bin".into()])),
            ..Default::default()
        };
        let merged = super::merge_resolved(c, l);
        match merged.entry_points {
            Some(crate::fixup::EntryPoints::Named(names)) => {
                assert_eq!(names, vec!["l-bin".to_string()]);
            }
            other => panic!("expected Named, got {:?}", other),
        }
    }

    #[test]
    fn merge_resolved_visibility_local_some_wins() {
        let c = ResolvedFixup {
            visibility: Some(vec!["//c:...".into()]),
            ..Default::default()
        };
        let l = ResolvedFixup {
            visibility: Some(vec!["//l:...".into()]),
            ..Default::default()
        };
        let merged = super::merge_resolved(c, l);
        assert_eq!(merged.visibility, Some(vec!["//l:...".to_string()]));
    }

    #[test]
    fn merge_resolved_labels_concat_dedup() {
        let c = ResolvedFixup {
            labels: vec!["tag-a".into(), "tag-b".into()],
            ..Default::default()
        };
        let l = ResolvedFixup {
            labels: vec!["tag-b".into(), "tag-c".into()],
            ..Default::default()
        };
        let merged = super::merge_resolved(c, l);
        assert_eq!(merged.labels, vec!["tag-a", "tag-b", "tag-c"]);
    }

    #[test]
    fn merge_resolved_runtime_env_local_overrides_key() {
        let mut c_re = std::collections::BTreeMap::new();
        c_re.insert("FOO".to_string(), "c".to_string());
        c_re.insert("BAR".to_string(), "c".to_string());
        let c = ResolvedFixup {
            runtime_env: c_re,
            ..Default::default()
        };
        let mut l_re = std::collections::BTreeMap::new();
        l_re.insert("FOO".to_string(), "l".to_string());
        l_re.insert("BAZ".to_string(), "l".to_string());
        let l = ResolvedFixup {
            runtime_env: l_re,
            ..Default::default()
        };
        let merged = super::merge_resolved(c, l);
        assert_eq!(merged.runtime_env.get("FOO").map(|s| s.as_str()), Some("l"));
        assert_eq!(merged.runtime_env.get("BAR").map(|s| s.as_str()), Some("c"));
        assert_eq!(merged.runtime_env.get("BAZ").map(|s| s.as_str()), Some("l"));
    }

    #[test]
    fn resolve_section_order_preserved() {
        // Later cfg sections override earlier scalar values.
        let (v, py) = empty_ctx();
        let ctx = CfgContext {
            package_version: &v,
            python_version: py,
            target_os: "linux",
            target_arch: "x86_64",
            target_env: "gnu",
        };
        let config = FixupConfig {
            top: FixupBody {
                overlay: Some("top".into()),
                ..Default::default()
            },
            cfg_sections: vec![
                (
                    "target_os = \"linux\"".into(),
                    FixupBody {
                        overlay: Some("first".into()),
                        ..Default::default()
                    },
                ),
                (
                    "target_arch = \"x86_64\"".into(),
                    FixupBody {
                        overlay: Some("second".into()),
                        ..Default::default()
                    },
                ),
            ],
            replace_community: false,
        };
        let resolved = resolve_for_cell(&config, &ctx);
        assert_eq!(
            resolved.overlay.as_deref(),
            Some(std::path::Path::new("second"))
        );
    }

    fn ctx_linux() -> CfgContext<'static> {
        use pep440_rs::Version;
        // We need static lifetime; use leaking for test ergonomics.
        let v: &'static Version = Box::leak(Box::new(Version::from_str("1.0").unwrap()));
        let py: Version = Version::from_str("3.12").unwrap();
        CfgContext {
            package_version: v,
            python_version: py,
            target_os: "linux",
            target_arch: "x86_64",
            target_env: "gnu",
        }
    }

    fn fixup_with(extra_deps: Vec<&str>) -> crate::fixup::FixupConfig {
        crate::fixup::FixupConfig {
            top: crate::fixup::FixupBody {
                extra_deps: extra_deps.into_iter().map(String::from).collect(),
                ..Default::default()
            },
            cfg_sections: vec![],
            replace_community: false,
        }
    }

    #[test]
    fn effective_fixups_resolve_returns_default_when_neither_layer_has_pkg() {
        use crate::fixup::loader::FixupSet;
        let eff = super::EffectiveFixups {
            community: FixupSet::default(),
            local: FixupSet::default(),
        };
        let pkg = pep508_rs::PackageName::from_str("missing").unwrap();
        let rf = eff.resolve(&pkg, &ctx_linux());
        assert!(rf.extra_deps.is_empty());
        assert!(rf.replace_deps.is_empty());
        assert!(rf.overlay.is_none());
    }

    #[test]
    fn effective_fixups_resolve_community_only() {
        use crate::fixup::loader::FixupSet;
        let mut comm = std::collections::BTreeMap::new();
        let pkg = pep508_rs::PackageName::from_str("pkg").unwrap();
        comm.insert(pkg.clone(), fixup_with(vec!["//c:base"]));
        let eff = super::EffectiveFixups {
            community: FixupSet::from_map_for_test(comm),
            local: FixupSet::default(),
        };
        let rf = eff.resolve(&pkg, &ctx_linux());
        assert_eq!(rf.extra_deps, vec!["//c:base"]);
    }

    #[test]
    fn effective_fixups_resolve_local_only() {
        use crate::fixup::loader::FixupSet;
        let mut loc = std::collections::BTreeMap::new();
        let pkg = pep508_rs::PackageName::from_str("pkg").unwrap();
        loc.insert(pkg.clone(), fixup_with(vec!["//l:base"]));
        let eff = super::EffectiveFixups {
            community: FixupSet::default(),
            local: FixupSet::from_map_for_test(loc),
        };
        let rf = eff.resolve(&pkg, &ctx_linux());
        assert_eq!(rf.extra_deps, vec!["//l:base"]);
    }

    #[test]
    fn effective_fixups_resolve_both_layers_concat() {
        use crate::fixup::loader::FixupSet;
        let pkg = pep508_rs::PackageName::from_str("pkg").unwrap();
        let mut comm = std::collections::BTreeMap::new();
        comm.insert(pkg.clone(), fixup_with(vec!["//c:base"]));
        let mut loc = std::collections::BTreeMap::new();
        loc.insert(pkg.clone(), fixup_with(vec!["//l:base"]));
        let eff = super::EffectiveFixups {
            community: FixupSet::from_map_for_test(comm),
            local: FixupSet::from_map_for_test(loc),
        };
        let rf = eff.resolve(&pkg, &ctx_linux());
        assert_eq!(rf.extra_deps, vec!["//c:base", "//l:base"]);
    }

    #[test]
    fn effective_fixups_resolve_replace_community_drops_community() {
        use crate::fixup::loader::FixupSet;
        let pkg = pep508_rs::PackageName::from_str("pkg").unwrap();
        let mut comm = std::collections::BTreeMap::new();
        comm.insert(pkg.clone(), fixup_with(vec!["//c:never-merged"]));

        let mut loc = std::collections::BTreeMap::new();
        let local_cfg = crate::fixup::FixupConfig {
            top: crate::fixup::FixupBody {
                extra_deps: vec!["//l:only".to_string()],
                ..Default::default()
            },
            cfg_sections: vec![],
            replace_community: true,
        };
        loc.insert(pkg.clone(), local_cfg);

        let eff = super::EffectiveFixups {
            community: FixupSet::from_map_for_test(comm),
            local: FixupSet::from_map_for_test(loc),
        };
        let rf = eff.resolve(&pkg, &ctx_linux());
        assert_eq!(rf.extra_deps, vec!["//l:only"]); // community discarded
    }

    #[test]
    fn effective_fixups_resolve_replace_community_with_empty_local_yields_empty() {
        use crate::fixup::loader::FixupSet;
        let pkg = pep508_rs::PackageName::from_str("pkg").unwrap();
        let mut comm = std::collections::BTreeMap::new();
        comm.insert(pkg.clone(), fixup_with(vec!["//c:base"]));

        let mut loc = std::collections::BTreeMap::new();
        let local_cfg = crate::fixup::FixupConfig {
            replace_community: true,
            ..Default::default()
        };
        loc.insert(pkg.clone(), local_cfg);

        let eff = super::EffectiveFixups {
            community: FixupSet::from_map_for_test(comm),
            local: FixupSet::from_map_for_test(loc),
        };
        let rf = eff.resolve(&pkg, &ctx_linux());
        assert!(rf.extra_deps.is_empty());
        assert!(rf.overlay.is_none());
    }

    #[test]
    fn effective_fixups_load_none_yields_empty_community() {
        use crate::fixup::RegistryConfig;
        use tempfile::TempDir;
        let tmp = TempDir::new().unwrap();
        let eff =
            super::EffectiveFixups::load(&RegistryConfig::None, tmp.path(), true).expect("loads");
        assert!(eff.community.is_empty());
        assert!(eff.local.is_empty()); // no fixups/ subdir
    }

    #[test]
    fn effective_fixups_load_file_url_walks_packages() {
        use crate::fixup::RegistryConfig;
        use tempfile::TempDir;

        let tmp = TempDir::new().unwrap();
        let registry_dir = tmp.path().join("registry");
        let tpd = tmp.path().join("third-party/python");
        std::fs::create_dir_all(registry_dir.join("packages/pillow")).unwrap();
        std::fs::write(
            registry_dir.join("packages/pillow/fixups.toml"),
            r#"extra_deps = ["//c:libjpeg"]"#,
        )
        .unwrap();
        std::fs::create_dir_all(&tpd).unwrap();

        let registry = RegistryConfig::FileUrl(registry_dir);
        let eff = super::EffectiveFixups::load(&registry, &tpd, true).expect("loads");
        let pillow = pep508_rs::PackageName::from_str("pillow").unwrap();
        assert!(eff.community.get(&pillow).is_some());
    }

    #[test]
    fn effective_fixups_load_file_url_missing_packages_errors() {
        use crate::fixup::RegistryConfig;
        use tempfile::TempDir;

        let tmp = TempDir::new().unwrap();
        let registry = RegistryConfig::FileUrl(tmp.path().to_path_buf());
        let err = super::EffectiveFixups::load(&registry, tmp.path(), true).unwrap_err();
        match err {
            crate::fixup::FixupError::RegistryPathNotFound { .. } => {}
            other => panic!("expected RegistryPathNotFound, got {:?}", other),
        }
    }

    #[test]
    fn effective_fixups_load_git_errors_in_s7a() {
        use crate::fixup::RegistryConfig;
        use tempfile::TempDir;

        let tmp = TempDir::new().unwrap();
        let registry = RegistryConfig::Git {
            url: "github.com/x/y".into(),
            rev: None,
        };
        let err = super::EffectiveFixups::load(&registry, tmp.path(), true).unwrap_err();
        match err {
            crate::fixup::FixupError::GitRegistryNotImplemented { registry } => {
                assert_eq!(registry, "github.com/x/y");
            }
            other => panic!("expected GitRegistryNotImplemented, got {:?}", other),
        }
    }

    #[test]
    fn effective_fixups_load_allow_local_overrides_false_skips_local() {
        use crate::fixup::RegistryConfig;
        use tempfile::TempDir;

        let tmp = TempDir::new().unwrap();
        let tpd = tmp.path().join("third-party/python");
        std::fs::create_dir_all(tpd.join("fixups/pillow")).unwrap();
        std::fs::write(
            tpd.join("fixups/pillow/fixups.toml"),
            r#"extra_deps = ["//local:pillow"]"#,
        )
        .unwrap();

        let eff = super::EffectiveFixups::load(&RegistryConfig::None, &tpd, false).expect("loads");
        assert!(eff.local.is_empty());
    }
}
