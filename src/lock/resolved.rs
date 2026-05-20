//! Per-(platform, python_version) projection of the dep graph.

use std::collections::{BTreeMap, BTreeSet};

use pep508_rs::{ExtraName, MarkerEnvironment};
use serde::Serialize;

use crate::config::{Config, Tree};
use crate::lock::graph::{DepGraph, NodeId, edge_applies, reachable_with_extras};
use crate::lock::types::Source;
use crate::platform::marker_env;

#[derive(Debug, Clone, Serialize)]
pub struct ResolvedView {
    pub configs: Vec<ResolvedConfig>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ResolvedConfig {
    pub platform: String,
    pub python_version: String,
    pub packages: Vec<ResolvedPackage>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ResolvedPackage {
    pub name: String,
    pub version: String,
    pub kind: ResolvedKind,
    #[serde(flatten)]
    pub source_info: SourceInfo,
    pub deps: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ResolvedKind {
    Registry,
    Git,
    FirstParty,
}

#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum SourceInfo {
    Registry {
        registry: String,
    },
    Git {
        git: String,
        rev: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        subdirectory: Option<String>,
    },
    FirstParty {
        first_party_kind: String,
        first_party_path: String,
    },
}

// ---- Public API ----

pub fn project(graph: &DepGraph, cfg: &Config, tree: &Tree) -> ResolvedView {
    let mut configs = Vec::new();
    for (platform_name, platform) in &cfg.platforms {
        for python in &tree.python_versions {
            let env = marker_env(platform, python.clone());
            let activation =
                reachable_with_extras(graph, &graph.roots, &env, &cfg.lockfile.include_groups);
            let packages = build_packages(graph, &activation, &env);
            configs.push(ResolvedConfig {
                platform: platform_name.clone(),
                python_version: format!("{}.{}", python.0, python.1),
                packages,
            });
        }
    }
    configs.sort_by(|a, b| {
        (a.platform.as_str(), a.python_version.as_str())
            .cmp(&(b.platform.as_str(), b.python_version.as_str()))
    });
    ResolvedView { configs }
}

fn build_packages(
    graph: &DepGraph,
    activation: &BTreeMap<NodeId, BTreeSet<ExtraName>>,
    env: &MarkerEnvironment,
) -> Vec<ResolvedPackage> {
    let mut packages: Vec<ResolvedPackage> = activation
        .iter()
        .map(|(&id, extras)| {
            let extras_vec: Vec<ExtraName> = extras.iter().cloned().collect();
            build_one(graph, id, env, &extras_vec)
        })
        .collect();
    packages.sort_by(|a, b| {
        kind_rank(a.kind)
            .cmp(&kind_rank(b.kind))
            .then_with(|| a.name.cmp(&b.name))
            .then_with(|| a.version.cmp(&b.version))
    });
    packages
}

fn kind_rank(k: ResolvedKind) -> u8 {
    match k {
        ResolvedKind::FirstParty => 0,
        ResolvedKind::Registry => 1,
        ResolvedKind::Git => 2,
    }
}

fn build_one(
    graph: &DepGraph,
    id: NodeId,
    env: &MarkerEnvironment,
    extras: &[ExtraName],
) -> ResolvedPackage {
    let n = &graph.nodes[id as usize];
    let kind = match &n.pkg.source {
        Source::Registry { .. } => ResolvedKind::Registry,
        Source::Git { .. } => ResolvedKind::Git,
        Source::FirstParty { .. } => ResolvedKind::FirstParty,
    };
    let source_info = match &n.pkg.source {
        Source::Registry { url } => SourceInfo::Registry {
            registry: url.to_string(),
        },
        Source::Git {
            url,
            rev,
            subdirectory,
        } => SourceInfo::Git {
            git: url.to_string(),
            rev: rev.clone(),
            subdirectory: subdirectory.clone(),
        },
        Source::FirstParty { kind, path } => SourceInfo::FirstParty {
            first_party_kind: format!("{kind:?}").to_lowercase(),
            first_party_path: path.clone(),
        },
    };
    let mut deps: Vec<String> = n
        .edges_out
        .iter()
        .enumerate()
        .filter(|(i, _)| {
            let marker = n.edge_markers[*i].as_ref();
            edge_applies(marker, env, extras)
        })
        .map(|(_, &target_id)| {
            let t = &graph.nodes[target_id as usize];
            format!("{}@{}", t.pkg.name, t.pkg.version)
        })
        .collect();
    deps.sort();
    deps.dedup();
    ResolvedPackage {
        name: n.pkg.name.as_ref().to_string(),
        version: n.pkg.version.to_string(),
        kind,
        source_info,
        deps,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{BuckConfig, FixupsConfig, LockfileConfig, Platform, PythonVersion};
    use crate::lock::graph::build;
    use crate::lock::types::*;
    use pep440_rs::Version;
    use pep508_rs::PackageName;
    use std::collections::BTreeMap;
    use std::str::FromStr;
    use url::Url;

    fn fixture_lock() -> Lockfile {
        Lockfile {
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
                        name: PackageName::from_str("numpy").unwrap(),
                        extra: vec![],
                        marker: None,
                    }],
                    sdist: None,
                    wheels: vec![],
                    metadata: None,
                },
                Package {
                    name: PackageName::from_str("numpy").unwrap(),
                    version: Version::from_str("2.4.6").unwrap(),
                    source: Source::Registry {
                        url: Url::parse("https://pypi.org/simple").unwrap(),
                    },
                    dependencies: vec![],
                    sdist: None,
                    wheels: vec![],
                    metadata: None,
                },
            ],
        }
    }

    fn fixture_config() -> (Config, Tree) {
        let mut platforms = BTreeMap::new();
        platforms.insert(
            "linux-x86_64-gnu".into(),
            Platform {
                target: "x86_64-unknown-linux-gnu".into(),
                manylinux: Some("2_17".into()),
                musllinux: None,
                macos_min: None,
            },
        );
        let tree = Tree {
            name: "default".into(),
            manifest_path: "pyproject.toml".into(),
            third_party_dir: ".".into(),
            python_versions: vec![PythonVersion(3, 12)],
        };
        let config = Config {
            trees: vec![tree.clone()],
            platforms,
            fixups: FixupsConfig::default(),
            buck: BuckConfig::default(),
            lockfile: LockfileConfig::default(),
        };
        (config, tree)
    }

    #[test]
    fn project_produces_one_config_per_combination() {
        let lock = fixture_lock();
        let g = build(&lock).expect("build");
        let (cfg, tree) = fixture_config();
        let view = project(&g, &cfg, &tree);
        assert_eq!(view.configs.len(), 1);
        let cfg0 = &view.configs[0];
        assert_eq!(cfg0.platform, "linux-x86_64-gnu");
        assert_eq!(cfg0.python_version, "3.12");
        assert_eq!(cfg0.packages.len(), 2);
        // First-party first, then registry packages alphabetically.
        assert_eq!(cfg0.packages[0].name, "app");
        assert!(matches!(cfg0.packages[0].kind, ResolvedKind::FirstParty));
        assert_eq!(cfg0.packages[1].name, "numpy");
        assert!(matches!(cfg0.packages[1].kind, ResolvedKind::Registry));
    }

    #[test]
    fn project_is_deterministic() {
        let lock = fixture_lock();
        let g = build(&lock).expect("build");
        let (cfg, tree) = fixture_config();
        let v1 = serde_json::to_string(&project(&g, &cfg, &tree)).unwrap();
        let v2 = serde_json::to_string(&project(&g, &cfg, &tree)).unwrap();
        assert_eq!(v1, v2);
    }
}
