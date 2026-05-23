//! Fixup TOML schema. `deny_unknown_fields` everywhere — community
//! fixup authors must use the v1 schema exactly; typos are errors.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// One package's fixup, as loaded from `fixups/<pkg>/fixups.toml`.
///
/// `top` is the body of the top-level fields; `cfg_sections` is the
/// ordered list of `['cfg(<expr>)']` sections, each paired with its
/// raw predicate string (parsed lazily by `cfg.rs`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FixupConfig {
    pub top: FixupBody,
    /// Raw `(predicate_string, body)` pairs in source order.
    pub cfg_sections: Vec<(String, FixupBody)>,
}

/// The body of either a top-level fixup or a single `cfg(...)` section.
#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, default)]
pub struct FixupBody {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub extra_deps: Vec<String>,

    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub omit_deps: Vec<String>,

    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub replace_deps: BTreeMap<String, String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub prefer_wheel: Option<String>,

    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub exclude_wheels: Vec<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub overlay: Option<PathBuf>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub entry_points: Option<EntryPoints>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub visibility: Option<Vec<String>>,

    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub labels: Vec<String>,

    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub runtime_env: BTreeMap<String, String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub sdist: Option<SdistFixup>,

    #[serde(skip_serializing_if = "is_false")]
    pub replace_community: bool,
}

fn is_false(b: &bool) -> bool {
    !b
}

/// `entry_points = true | ["name1", "name2"]`. v1: only the list form
/// can be applied; the `true` shorthand parses but errors at apply.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(untagged)]
pub enum EntryPoints {
    Auto(bool),
    Named(Vec<String>),
}

/// v2 sdist-build surface. Schema-only in S6 — parsed but not applied.
#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, default)]
pub struct SdistFixup {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backend: Option<String>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub build_env: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub build_deps: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub extra_native_libs: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub data_files: Vec<String>,
}

impl FixupConfig {
    /// Parse a fixup TOML document into top-level body + cfg sections.
    ///
    /// `[cfg(...)]` sections are read by collecting top-level table
    /// keys that begin with `cfg(` (the TOML section header IS the key
    /// at the top level). The remaining keys flatten into `FixupBody`.
    pub fn from_toml_str(s: &str) -> Result<Self, toml::de::Error> {
        // toml -> typed shim that captures every key.
        let raw: toml::Table = toml::from_str(s)?;
        let mut top_table = toml::Table::new();
        let mut cfg_sections: Vec<(String, FixupBody)> = Vec::new();

        for (key, value) in raw {
            if let Some(predicate) = key.strip_prefix("cfg(").and_then(|t| t.strip_suffix(')')) {
                let body: FixupBody = value.try_into()?;
                cfg_sections.push((predicate.to_string(), body));
            } else {
                top_table.insert(key, value);
            }
        }

        let top: FixupBody = toml::Value::Table(top_table).try_into()?;
        Ok(FixupConfig { top, cfg_sections })
    }

    /// Re-emit as canonical TOML. Round-trippable up to formatting
    /// (key ordering, default omission).
    pub fn to_toml_string(&self) -> Result<String, toml::ser::Error> {
        // Use serde's flattening: emit top fields, then each cfg section.
        let mut out = toml::to_string_pretty(&self.top)?;
        for (predicate, body) in &self.cfg_sections {
            // Skip empty bodies (would emit just a header).
            let body_str = toml::to_string_pretty(body)?;
            if body_str.trim().is_empty() {
                continue;
            }
            out.push_str(&format!("\n[\"cfg({})\"]\n", predicate));
            out.push_str(&body_str);
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_minimal_fixup() {
        let toml = r#"
            extra_deps = ["//third-party/c:libjpeg"]
            omit_deps = ["useless"]
        "#;
        let cfg = FixupConfig::from_toml_str(toml).unwrap();
        assert_eq!(cfg.top.extra_deps, vec!["//third-party/c:libjpeg"]);
        assert_eq!(cfg.top.omit_deps, vec!["useless"]);
        assert!(cfg.cfg_sections.is_empty());
    }

    #[test]
    fn parses_with_cfg_section() {
        let toml = r#"
            extra_deps = ["//a:b"]

            ["cfg(target_os = \"linux\")"]
            extra_deps = ["//c:d"]
        "#;
        let cfg = FixupConfig::from_toml_str(toml).unwrap();
        assert_eq!(cfg.cfg_sections.len(), 1);
        assert_eq!(cfg.cfg_sections[0].0, "target_os = \"linux\"");
        assert_eq!(cfg.cfg_sections[0].1.extra_deps, vec!["//c:d"]);
    }

    #[test]
    fn rejects_unknown_top_level_field() {
        let toml = r#"
            extra_things = ["x"]
        "#;
        let err = FixupConfig::from_toml_str(toml).unwrap_err();
        assert!(err.to_string().contains("extra_things"), "got: {}", err);
    }

    #[test]
    fn parses_entry_points_list() {
        let toml = r#"entry_points = ["ruff", "ruff-lsp"]"#;
        let cfg = FixupConfig::from_toml_str(toml).unwrap();
        match cfg.top.entry_points {
            Some(EntryPoints::Named(v)) => assert_eq!(v, vec!["ruff", "ruff-lsp"]),
            other => panic!("expected Named, got {:?}", other),
        }
    }

    #[test]
    fn parses_entry_points_true() {
        let toml = r#"entry_points = true"#;
        let cfg = FixupConfig::from_toml_str(toml).unwrap();
        match cfg.top.entry_points {
            Some(EntryPoints::Auto(true)) => {}
            other => panic!("expected Auto(true), got {:?}", other),
        }
    }

    #[test]
    fn parses_replace_deps() {
        let toml = r#"replace_deps = { numpy = "//company/numpy:numpy" }"#;
        let cfg = FixupConfig::from_toml_str(toml).unwrap();
        assert_eq!(
            cfg.top.replace_deps.get("numpy").map(|s| s.as_str()),
            Some("//company/numpy:numpy")
        );
    }

    #[test]
    fn parses_replace_community_default_false() {
        let toml = r#"extra_deps = ["//x:y"]"#;
        let cfg = FixupConfig::from_toml_str(toml).unwrap();
        assert!(!cfg.top.replace_community);
    }

    #[test]
    fn parses_replace_community_true() {
        let toml = r#"replace_community = true"#;
        let cfg = FixupConfig::from_toml_str(toml).unwrap();
        assert!(cfg.top.replace_community);
    }

    #[test]
    fn parses_sdist_block() {
        let toml = r#"
            [sdist]
            backend = "maturin"
            build_env = { FOO = "bar" }
        "#;
        let cfg = FixupConfig::from_toml_str(toml).unwrap();
        let sdist = cfg.top.sdist.unwrap();
        assert_eq!(sdist.backend.as_deref(), Some("maturin"));
        assert_eq!(sdist.build_env.get("FOO").map(|s| s.as_str()), Some("bar"));
    }

    #[test]
    fn round_trip_preserves_user_set_values() {
        let toml = r#"
            extra_deps = ["//a:b"]
            labels = ["x"]
            entry_points = ["bin"]
        "#;
        let cfg = FixupConfig::from_toml_str(toml).unwrap();
        let out = cfg.to_toml_string().unwrap();
        let cfg2 = FixupConfig::from_toml_str(&out).unwrap();
        assert_eq!(cfg, cfg2);
    }
}
