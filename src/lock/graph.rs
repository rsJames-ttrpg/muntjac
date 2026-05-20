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

pub fn detect_cycles(graph: &DepGraph) -> Result<(), LockfileError> {
    let sccs = tarjan_scc(graph);
    let cycles: Vec<String> = sccs
        .iter()
        .filter(|scc| scc.len() > 1 || has_self_loop(graph, scc[0]))
        .map(|scc| format_cycle(graph, scc))
        .collect();
    if cycles.is_empty() {
        return Ok(());
    }
    Err(LockfileError::Cycle(cycles))
}

fn has_self_loop(graph: &DepGraph, id: NodeId) -> bool {
    graph.nodes[id as usize].edges_out.contains(&id)
}

fn format_cycle(graph: &DepGraph, scc: &[NodeId]) -> String {
    let mut sorted = scc.to_vec();
    sorted.sort();
    sorted
        .iter()
        .map(|&id| {
            let n = &graph.nodes[id as usize];
            format!("{}@{}", n.pkg.name, n.pkg.version)
        })
        .collect::<Vec<_>>()
        .join(" -> ")
}

fn tarjan_scc(graph: &DepGraph) -> Vec<Vec<NodeId>> {
    let n = graph.nodes.len();
    let mut index = vec![-1i32; n];
    let mut lowlink = vec![0i32; n];
    let mut on_stack = vec![false; n];
    let mut stack: Vec<NodeId> = Vec::new();
    let mut next_index = 0i32;
    let mut sccs: Vec<Vec<NodeId>> = Vec::new();

    for v in 0..n {
        if index[v] == -1 {
            strongconnect(
                v,
                graph,
                &mut index,
                &mut lowlink,
                &mut on_stack,
                &mut stack,
                &mut next_index,
                &mut sccs,
            );
        }
    }
    sccs
}

#[allow(clippy::too_many_arguments)]
fn strongconnect(
    v: usize,
    graph: &DepGraph,
    index: &mut [i32],
    lowlink: &mut [i32],
    on_stack: &mut [bool],
    stack: &mut Vec<NodeId>,
    next_index: &mut i32,
    sccs: &mut Vec<Vec<NodeId>>,
) {
    index[v] = *next_index;
    lowlink[v] = *next_index;
    *next_index += 1;
    stack.push(v as NodeId);
    on_stack[v] = true;

    let successors = graph.nodes[v].edges_out.clone();
    for w in successors {
        let w = w as usize;
        if index[w] == -1 {
            strongconnect(w, graph, index, lowlink, on_stack, stack, next_index, sccs);
            lowlink[v] = lowlink[v].min(lowlink[w]);
        } else if on_stack[w] {
            lowlink[v] = lowlink[v].min(index[w]);
        }
    }

    if lowlink[v] == index[v] {
        let mut scc = Vec::new();
        loop {
            let w = stack.pop().unwrap();
            on_stack[w as usize] = false;
            scc.push(w);
            if w as usize == v {
                break;
            }
        }
        sccs.push(scc);
    }
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

    #[test]
    fn detects_self_loop() {
        let lock = Lockfile {
            version: 1,
            revision: 3,
            requires_python: ">=3.12".into(),
            packages: vec![pkg("a", "1.0", first_party(), vec!["a"])],
        };
        let g = build(&lock).expect("build");
        let err = detect_cycles(&g).expect_err("should fail");
        match err {
            LockfileError::Cycle(cycles) => {
                assert_eq!(cycles.len(), 1);
                assert!(cycles[0].contains("a@1.0"));
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn detects_three_cycle() {
        let lock = Lockfile {
            version: 1,
            revision: 3,
            requires_python: ">=3.12".into(),
            packages: vec![
                pkg("a", "1.0", first_party(), vec!["b"]),
                pkg("b", "1.0", registry(), vec!["c"]),
                pkg("c", "1.0", registry(), vec!["a"]),
            ],
        };
        let g = build(&lock).expect("build");
        let err = detect_cycles(&g).expect_err("should fail");
        let s = format!("{err}");
        assert!(s.contains("a@1.0") && s.contains("b@1.0") && s.contains("c@1.0"));
    }

    #[test]
    fn passes_acyclic_diamond() {
        let lock = Lockfile {
            version: 1,
            revision: 3,
            requires_python: ">=3.12".into(),
            packages: vec![
                pkg("a", "1.0", first_party(), vec!["b", "c"]),
                pkg("b", "1.0", registry(), vec!["d"]),
                pkg("c", "1.0", registry(), vec!["d"]),
                pkg("d", "1.0", registry(), vec![]),
            ],
        };
        let g = build(&lock).expect("build");
        detect_cycles(&g).expect("acyclic");
    }
}
