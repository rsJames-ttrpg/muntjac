//! Merge multiple `FixupBody` instances into a single `ResolvedFixup`.
//!
//! S6 is local-only: `resolve_for_cell` runs `merge_into` over the
//! top-level body, then over each cfg section whose predicate matches.
//! S7 will add the community layer as an outer wrapper.

use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::fixup::cfg::{CfgContext, CfgPredicate};
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
        };
        let resolved = resolve_for_cell(&config, &ctx);
        assert_eq!(resolved.extra_deps, vec!["//base:dep"]);
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
        };
        let resolved = resolve_for_cell(&config, &ctx);
        assert_eq!(
            resolved.overlay.as_deref(),
            Some(std::path::Path::new("second"))
        );
    }
}
