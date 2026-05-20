//! Dependency graph built from a parsed Lockfile.
//!
//! Adjacency-map representation. Cycle detection (Task 9) and reachability
//! (Task 10) are added by later tasks in this stage.

use std::collections::BTreeMap;

use pep508_rs::{MarkerTree, PackageName};

use crate::error::LockfileError;
use crate::lock::types::*;

pub type NodeId = u32;

#[derive(Debug, Clone)]
pub struct DepGraph {
    pub nodes: Vec<GraphNode>,
    pub by_name: BTreeMap<PackageName, NodeId>,
    pub roots: Vec<NodeId>,
}

#[derive(Debug, Clone)]
pub struct GraphNode {
    pub pkg: Package,
    pub edges_out: Vec<NodeId>,
    pub edge_markers: Vec<Option<MarkerTree>>,
    pub edge_extras: Vec<Vec<String>>,
}

// ---- Public API ----

pub fn build(lock: &Lockfile) -> Result<DepGraph, LockfileError> {
    // Pass 1: allocate nodes and detect duplicate names.
    let mut nodes: Vec<GraphNode> = Vec::with_capacity(lock.packages.len());
    let mut by_name: BTreeMap<PackageName, NodeId> = BTreeMap::new();
    let mut dup_versions: BTreeMap<PackageName, Vec<String>> = BTreeMap::new();

    for pkg in &lock.packages {
        let id = nodes.len() as NodeId;
        if let Some(&existing_id) = by_name.get(&pkg.name) {
            let existing = &nodes[existing_id as usize];
            let entry = dup_versions
                .entry(pkg.name.clone())
                .or_insert_with(|| vec![existing.pkg.version.to_string()]);
            entry.push(pkg.version.to_string());
        }
        nodes.push(GraphNode {
            pkg: pkg.clone(),
            edges_out: Vec::new(),
            edge_markers: Vec::new(),
            edge_extras: Vec::new(),
        });
        by_name.insert(pkg.name.clone(), id);
    }

    if let Some((name, versions)) = dup_versions.into_iter().next() {
        return Err(LockfileError::DuplicatePackageName {
            name: name.as_ref().to_string(),
            versions,
        });
    }

    // Pass 2: resolve edges.
    for id in 0..(nodes.len() as NodeId) {
        let edges = nodes[id as usize].pkg.dependencies.clone();
        let source_name = nodes[id as usize].pkg.name.clone();
        for edge in edges {
            let &target = by_name
                .get(&edge.name)
                .ok_or_else(|| LockfileError::UnresolvedDep {
                    from_pkg: source_name.as_ref().to_string(),
                    dep: edge.name.as_ref().to_string(),
                })?;
            nodes[id as usize].edges_out.push(target);
            nodes[id as usize].edge_markers.push(edge.marker);
            nodes[id as usize].edge_extras.push(edge.extra);
        }
    }

    // Roots: first-party packages.
    let roots: Vec<NodeId> = nodes
        .iter()
        .enumerate()
        .filter(|(_, n)| matches!(n.pkg.source, Source::FirstParty { .. }))
        .map(|(i, _)| i as NodeId)
        .collect();

    Ok(DepGraph {
        nodes,
        by_name,
        roots,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use pep440_rs::Version;
    use std::str::FromStr;
    use url::Url;

    fn pkg(name: &str, version: &str, source: Source, deps: Vec<&str>) -> Package {
        Package {
            name: PackageName::from_str(name).unwrap(),
            version: Version::from_str(version).unwrap(),
            source,
            dependencies: deps
                .into_iter()
                .map(|n| DepEdge {
                    name: PackageName::from_str(n).unwrap(),
                    extra: vec![],
                    marker: None,
                })
                .collect(),
            sdist: None,
            wheels: vec![],
            metadata: None,
        }
    }

    fn registry() -> Source {
        Source::Registry {
            url: Url::parse("https://pypi.org/simple").unwrap(),
        }
    }
    fn first_party() -> Source {
        Source::FirstParty {
            kind: FirstPartyKind::Virtual,
            path: ".".into(),
        }
    }

    #[test]
    fn builds_linear_chain() {
        let lock = Lockfile {
            version: 1,
            revision: 3,
            requires_python: ">=3.12".into(),
            packages: vec![
                pkg("a", "1.0", first_party(), vec!["b"]),
                pkg("b", "1.0", registry(), vec!["c"]),
                pkg("c", "1.0", registry(), vec![]),
            ],
        };
        let g = build(&lock).expect("build");
        assert_eq!(g.nodes.len(), 3);
        assert_eq!(g.roots.len(), 1);
        let a_id = g.by_name[&PackageName::from_str("a").unwrap()];
        let b_id = g.by_name[&PackageName::from_str("b").unwrap()];
        assert_eq!(g.nodes[a_id as usize].edges_out, vec![b_id]);
    }

    #[test]
    fn errors_on_duplicate_package_name() {
        let lock = Lockfile {
            version: 1,
            revision: 3,
            requires_python: ">=3.12".into(),
            packages: vec![
                pkg("numpy", "1.0", registry(), vec![]),
                pkg("numpy", "2.0", registry(), vec![]),
            ],
        };
        let err = build(&lock).expect_err("should fail");
        assert!(matches!(err, LockfileError::DuplicatePackageName { .. }));
    }

    #[test]
    fn errors_on_unresolved_dep() {
        let lock = Lockfile {
            version: 1,
            revision: 3,
            requires_python: ">=3.12".into(),
            packages: vec![pkg("a", "1.0", first_party(), vec!["ghost"])],
        };
        let err = build(&lock).expect_err("should fail");
        // NOTE: field is from_pkg, not source (thiserror 2.0 reserved that name)
        match err {
            LockfileError::UnresolvedDep { from_pkg, dep } => {
                assert_eq!(from_pkg, "a");
                assert_eq!(dep, "ghost");
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }
}
