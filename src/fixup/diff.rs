//! Structural diff between two `FixupSet`s for `muntjac fixups update`.

use std::collections::BTreeSet;

use pep508_rs::PackageName;

use crate::fixup::{FixupConfig, FixupSet};

#[derive(Debug, PartialEq, Eq)]
pub enum DiffLine {
    /// Package present in `new`, absent in `old`.
    Added(PackageName),
    /// Package present in `old`, absent in `new`.
    Removed(PackageName),
    /// Package present in both with different content. The string list
    /// names which fields differ (canonical order, see `modified_fields`).
    Modified(PackageName, Vec<&'static str>),
}

/// Compare two FixupSets. Returns DiffLines in canonical package-name order
/// (lexicographic on the PEP-503-normalized form).
pub fn diff_fixup_sets(old: &FixupSet, new: &FixupSet) -> Vec<DiffLine> {
    let old_names: BTreeSet<&PackageName> = old.iter().map(|(n, _)| n).collect();
    let new_names: BTreeSet<&PackageName> = new.iter().map(|(n, _)| n).collect();
    let all: BTreeSet<&PackageName> = old_names.union(&new_names).copied().collect();

    let mut out = Vec::new();
    for name in all {
        match (old.get(name), new.get(name)) {
            (None, Some(_)) => out.push(DiffLine::Added(name.clone())),
            (Some(_), None) => out.push(DiffLine::Removed(name.clone())),
            (Some(o), Some(n)) => {
                let fields = modified_fields(o, n);
                if !fields.is_empty() {
                    out.push(DiffLine::Modified(name.clone(), fields));
                }
            }
            (None, None) => unreachable!(),
        }
    }
    out
}

/// Field-by-field structural compare. Returns the names of fields that differ.
fn modified_fields(a: &FixupConfig, b: &FixupConfig) -> Vec<&'static str> {
    let mut fields = Vec::new();

    if a.top.extra_deps != b.top.extra_deps {
        fields.push("extra_deps");
    }
    if a.top.omit_deps != b.top.omit_deps {
        fields.push("omit_deps");
    }
    if a.top.replace_deps != b.top.replace_deps {
        fields.push("replace_deps");
    }
    if a.top.prefer_wheel != b.top.prefer_wheel {
        fields.push("prefer_wheel");
    }
    if a.top.exclude_wheels != b.top.exclude_wheels {
        fields.push("exclude_wheels");
    }
    if a.top.overlay != b.top.overlay {
        fields.push("overlay");
    }
    if a.top.entry_points != b.top.entry_points {
        fields.push("entry_points");
    }
    if a.top.visibility != b.top.visibility {
        fields.push("visibility");
    }
    if a.top.labels != b.top.labels {
        fields.push("labels");
    }
    if a.top.runtime_env != b.top.runtime_env {
        fields.push("runtime_env");
    }
    if a.top.sdist != b.top.sdist {
        fields.push("sdist");
    }
    if a.replace_community != b.replace_community {
        fields.push("replace_community");
    }
    if a.cfg_sections != b.cfg_sections {
        fields.push("cfg_sections");
    }

    fields
}

/// Render diff lines as the user-facing text format.
/// Returns empty string for empty input, else newline-terminated.
pub fn render_diff(lines: &[DiffLine]) -> String {
    if lines.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    for line in lines {
        match line {
            DiffLine::Added(n) => {
                out.push_str(&format!("+ {}\n", n));
            }
            DiffLine::Removed(n) => {
                out.push_str(&format!("- {}\n", n));
            }
            DiffLine::Modified(n, fields) => {
                out.push_str(&format!("~ {} ({})\n", n, fields.join(", ")));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixup::{FixupBody, FixupConfig, FixupSet};
    use std::collections::BTreeMap;
    use std::str::FromStr;

    fn fixup_with_extra_deps(deps: Vec<&str>) -> FixupConfig {
        FixupConfig {
            top: FixupBody {
                extra_deps: deps.into_iter().map(String::from).collect(),
                ..Default::default()
            },
            cfg_sections: vec![],
            replace_community: false,
        }
    }

    fn set(pkgs: Vec<(&str, FixupConfig)>) -> FixupSet {
        let mut m = BTreeMap::new();
        for (n, c) in pkgs {
            m.insert(PackageName::from_str(n).unwrap(), c);
        }
        FixupSet::from_map_for_test(m)
    }

    #[test]
    fn diff_added_package() {
        let old = set(vec![]);
        let new = set(vec![("pkg-a", fixup_with_extra_deps(vec!["//x:y"]))]);
        let lines = diff_fixup_sets(&old, &new);
        assert_eq!(lines.len(), 1);
        assert!(matches!(&lines[0], DiffLine::Added(n) if n.as_ref() == "pkg-a"));
    }

    #[test]
    fn diff_removed_package() {
        let old = set(vec![("pkg-a", fixup_with_extra_deps(vec!["//x:y"]))]);
        let new = set(vec![]);
        let lines = diff_fixup_sets(&old, &new);
        assert_eq!(lines.len(), 1);
        assert!(matches!(&lines[0], DiffLine::Removed(n) if n.as_ref() == "pkg-a"));
    }

    #[test]
    fn diff_modified_extra_deps() {
        let old = set(vec![("pkg-a", fixup_with_extra_deps(vec!["//x:y"]))]);
        let new = set(vec![(
            "pkg-a",
            fixup_with_extra_deps(vec!["//x:y", "//z:w"]),
        )]);
        let lines = diff_fixup_sets(&old, &new);
        assert_eq!(lines.len(), 1);
        match &lines[0] {
            DiffLine::Modified(n, fields) => {
                assert_eq!(n.as_ref(), "pkg-a");
                assert_eq!(fields, &vec!["extra_deps"]);
            }
            other => panic!("expected Modified, got {:?}", other),
        }
    }

    #[test]
    fn diff_modified_multiple_fields() {
        let mut a = fixup_with_extra_deps(vec!["//x:y"]);
        a.top.visibility = Some(vec!["//a:...".into()]);
        let mut b = fixup_with_extra_deps(vec!["//z:w"]);
        b.top.visibility = Some(vec!["//b:...".into()]);

        let old = set(vec![("pkg-a", a)]);
        let new = set(vec![("pkg-a", b)]);
        let lines = diff_fixup_sets(&old, &new);
        match &lines[0] {
            DiffLine::Modified(_, fields) => {
                assert_eq!(fields, &vec!["extra_deps", "visibility"]);
            }
            other => panic!("expected Modified, got {:?}", other),
        }
    }

    #[test]
    fn diff_modified_cfg_sections() {
        let a = fixup_with_extra_deps(vec!["//x:y"]);
        let mut b = fixup_with_extra_deps(vec!["//x:y"]);
        b.cfg_sections.push((
            "target_os = \"linux\"".to_string(),
            FixupBody {
                extra_deps: vec!["//linux:dep".into()],
                ..Default::default()
            },
        ));
        let old = set(vec![("pkg-a", a)]);
        let new = set(vec![("pkg-a", b)]);
        let lines = diff_fixup_sets(&old, &new);
        match &lines[0] {
            DiffLine::Modified(_, fields) => {
                assert_eq!(fields, &vec!["cfg_sections"]);
            }
            other => panic!("expected Modified, got {:?}", other),
        }
    }

    #[test]
    fn diff_identical_yields_nothing() {
        let cfg = fixup_with_extra_deps(vec!["//x:y"]);
        let old = set(vec![("pkg-a", cfg.clone())]);
        let new = set(vec![("pkg-a", cfg)]);
        let lines = diff_fixup_sets(&old, &new);
        assert!(lines.is_empty());
    }

    #[test]
    fn render_diff_canonical_format() {
        let lines = vec![
            DiffLine::Added(PackageName::from_str("aaa").unwrap()),
            DiffLine::Modified(
                PackageName::from_str("bbb").unwrap(),
                vec!["extra_deps", "visibility"],
            ),
            DiffLine::Removed(PackageName::from_str("ccc").unwrap()),
        ];
        let out = render_diff(&lines);
        assert_eq!(out, "+ aaa\n~ bbb (extra_deps, visibility)\n- ccc\n");
    }

    #[test]
    fn render_diff_empty_input_empty_output() {
        let lines: Vec<DiffLine> = vec![];
        let out = render_diff(&lines);
        assert_eq!(out, "");
    }
}
