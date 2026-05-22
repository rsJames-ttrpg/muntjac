//! Dependency graph built from a parsed Lockfile.
//!
//! Adjacency-map representation. Cycle detection (Task 9) and reachability
//! (Task 10) are added by later tasks in this stage.

use std::collections::{BTreeMap, BTreeSet};
use std::str::FromStr;

use pep508_rs::{ExtraName, MarkerEnvironment, MarkerTree, PackageName};

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

pub fn reachable_from(
    graph: &DepGraph,
    roots: &[NodeId],
    env: &MarkerEnvironment,
    include_groups: &[String],
) -> BTreeSet<NodeId> {
    reachable_with_extras(graph, roots, env, include_groups)
        .into_keys()
        .collect()
}

/// Computes the set of reachable nodes for a given `(platform, python_version)`,
/// activating extras either globally (from `include_groups`) or per-target node.
///
/// ## Two asymmetric extras sources
///
/// 1. **`include_groups` (global, every-node):** dependency-groups declared in
///    `muntjac.toml`'s `[lockfile] include_groups` apply to EVERY node in the
///    graph. A `tool.uv.dev-dependencies` group named `"test"` becomes a global
///    activation: every package's `[package.dev-dependencies.test]` edges fire.
///
/// 2. **`DepEdge.extra` (target-side, per-node):** when a package `A` depends on
///    `B[extra]`, the `[extra]` activation lives on `B`'s outgoing edges, not on
///    `A`'s. `A` doesn't gain anything from declaring the extra; only `B`'s
///    `[package.optional-dependencies.extra]` edges activate.
///
/// ## Worked example
///
/// Suppose `muntjac.toml` has `include_groups = ["test"]`, and the graph has:
///
/// ```text
/// app  ──> requests[security]
/// app  ──> pytest    (gated by `extra == 'test'` marker)
/// requests ──> certifi
/// requests ──> chardet    (gated by `extra == 'security'`)
/// pytest   ──> iniconfig
/// ```
///
/// `reachable_with_extras` activates:
/// - `app` → `requests`, `pytest` (the latter because `test` is global)
/// - `requests` → `certifi`, `chardet` (the latter because `app` requested `security`)
/// - `pytest` → `iniconfig`
///
/// Note that `app`'s `[package.dev-dependencies.test]` edges fire (global), but
/// `app` doesn't get `security`-gated edges of its own from declaring `requests[security]`.
pub fn reachable_with_extras(
    graph: &DepGraph,
    roots: &[NodeId],
    env: &MarkerEnvironment,
    include_groups: &[String],
) -> BTreeMap<NodeId, BTreeSet<ExtraName>> {
    let group_extras: BTreeSet<ExtraName> = include_groups
        .iter()
        .filter_map(|s| ExtraName::from_str(s).ok())
        .collect();

    let mut activated: BTreeMap<NodeId, BTreeSet<ExtraName>> = BTreeMap::new();
    let mut stack: Vec<NodeId> = Vec::new();
    for &root in roots {
        if activated.contains_key(&root) {
            continue;
        }
        activated.insert(root, group_extras.clone());
        stack.push(root);
    }

    while let Some(node) = stack.pop() {
        let node_extras: Vec<ExtraName> = activated
            .get(&node)
            .map(|s| s.iter().cloned().collect())
            .unwrap_or_default();
        let n = &graph.nodes[node as usize];
        for (i, &target) in n.edges_out.iter().enumerate() {
            let marker = n.edge_markers[i].as_ref();
            if !edge_applies(marker, env, &node_extras) {
                continue;
            }
            let edge_extras = &n.edge_extras[i];
            let newly_visited = !activated.contains_key(&target);
            let target_entry = activated.entry(target).or_default();
            let mut activated_new_extra = false;
            for e in &group_extras {
                if target_entry.insert(e.clone()) {
                    activated_new_extra = true;
                }
            }
            for s in edge_extras {
                if let Ok(name) = ExtraName::from_str(s)
                    && target_entry.insert(name)
                {
                    activated_new_extra = true;
                }
            }
            // Re-enqueue when newly visited or when newly activated extras
            // could unlock additional outgoing edges.
            if newly_visited || activated_new_extra {
                stack.push(target);
            }
        }
    }
    activated
}

/// Returns true if an edge with the given marker should be followed under
/// the given environment + extras (dependency-group activations).
pub fn edge_applies(
    marker: Option<&MarkerTree>,
    env: &MarkerEnvironment,
    extras: &[ExtraName],
) -> bool {
    let Some(m) = marker else {
        return true;
    };
    m.evaluate(env, extras)
}

pub fn detect_cycles(graph: &DepGraph) -> Result<(), LockfileError> {
    let sccs = tarjan_scc(graph);
    let mut out: Vec<Vec<String>> = Vec::new();
    for scc in sccs {
        let is_cycle = scc.len() > 1 || has_self_loop(graph, scc[0]);
        if !is_cycle {
            continue;
        }

        // Find the lex-smallest member by display string.
        let mut members: Vec<NodeId> = scc.clone();
        members.sort_by_key(|&n| display_string(graph, n));
        let start = members[0];

        // Walk along outgoing edges, preferring unvisited successors in the SCC.
        let scc_set: std::collections::HashSet<NodeId> = scc.iter().copied().collect();
        let mut path: Vec<NodeId> = vec![start];
        let mut visited: std::collections::HashSet<NodeId> = [start].into_iter().collect();

        loop {
            let current = *path.last().unwrap();
            let edges = &graph.nodes[current as usize].edges_out;
            let next = edges
                .iter()
                .filter(|&&succ| scc_set.contains(&succ))
                .find(|&&succ| !visited.contains(&succ))
                .or_else(|| edges.iter().find(|&&succ| scc_set.contains(&succ)));
            match next {
                Some(&n) if !visited.contains(&n) => {
                    path.push(n);
                    visited.insert(n);
                }
                Some(_) | None => break,
            }
        }

        let path_strs: Vec<String> = path.into_iter().map(|n| display_string(graph, n)).collect();
        out.push(path_strs);
    }
    out.sort();
    if out.is_empty() {
        return Ok(());
    }
    Err(LockfileError::Cycle(out))
}

fn has_self_loop(graph: &DepGraph, id: NodeId) -> bool {
    graph.nodes[id as usize].edges_out.contains(&id)
}

fn display_string(graph: &DepGraph, id: NodeId) -> String {
    let n = &graph.nodes[id as usize];
    format!("{}@{}", n.pkg.name, n.pkg.version)
}

fn tarjan_scc(graph: &DepGraph) -> Vec<Vec<NodeId>> {
    // Iterative Tarjan's SCC. The recursive form risks stack overflow on deep
    // linear dependency chains; this implementation uses an explicit work
    // stack of (node, edge-cursor) frames. Algorithm output is identical to
    // the recursive version: it visits successors in the same order, computes
    // the same lowlink/index values, and emits SCCs in the same reverse-
    // topological order.
    let n = graph.nodes.len();
    let mut index = vec![-1i32; n];
    let mut lowlink = vec![0i32; n];
    let mut on_stack = vec![false; n];
    let mut scc_stack: Vec<NodeId> = Vec::new();
    let mut next_index = 0i32;
    let mut sccs: Vec<Vec<NodeId>> = Vec::new();

    // Each work frame is (node, next-edge-index-to-process). The cursor lets
    // us resume the neighbor loop after a child finishes. Storing only the
    // cursor (instead of an owning iterator) avoids cloning edge lists.
    let mut work: Vec<(usize, usize)> = Vec::new();

    for start in 0..n {
        if index[start] != -1 {
            continue;
        }

        // "Enter" the start node, then drive the work loop.
        index[start] = next_index;
        lowlink[start] = next_index;
        next_index += 1;
        scc_stack.push(start as NodeId);
        on_stack[start] = true;
        work.push((start, 0));

        while let Some(&(v, edge_idx)) = work.last() {
            let edges = &graph.nodes[v].edges_out;
            if edge_idx < edges.len() {
                let w = edges[edge_idx] as usize;
                // Advance the cursor so when this frame is revisited we move
                // to the next neighbor.
                work.last_mut().unwrap().1 = edge_idx + 1;

                if index[w] == -1 {
                    // "Recurse" on w: enter it, then push its work frame.
                    index[w] = next_index;
                    lowlink[w] = next_index;
                    next_index += 1;
                    scc_stack.push(w as NodeId);
                    on_stack[w] = true;
                    work.push((w, 0));
                } else if on_stack[w] {
                    // Cross-edge to an on-stack ancestor: tighten v's lowlink.
                    lowlink[v] = lowlink[v].min(index[w]);
                }
                // else: edge to a finished SCC, ignored.
            } else {
                // All neighbors of v processed — close v out.
                if lowlink[v] == index[v] {
                    let mut scc = Vec::new();
                    loop {
                        let popped = scc_stack.pop().unwrap();
                        on_stack[popped as usize] = false;
                        scc.push(popped);
                        if popped as usize == v {
                            break;
                        }
                    }
                    sccs.push(scc);
                }

                // Pop v's frame, then propagate its lowlink up to its parent
                // (which is now the top of the work stack, if any). This is
                // the iterative analog of `lowlink[parent] = min(lowlink[parent],
                // lowlink[v])` after the recursive call returned.
                work.pop();
                if let Some(&(parent, _)) = work.last() {
                    lowlink[parent] = lowlink[parent].min(lowlink[v]);
                }
            }
        }
    }
    sccs
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
                assert_eq!(cycles[0], vec!["a@1.0"]);
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
        assert!(s.contains("dependency cycle(s) detected"));
    }

    #[test]
    fn detect_cycles_finds_simple_cycle() {
        // alpha -> beta -> alpha
        let lock = Lockfile {
            version: 1,
            revision: 3,
            requires_python: ">=3.12".into(),
            packages: vec![
                pkg("alpha", "1.0", first_party(), vec!["beta"]),
                pkg("beta", "1.0", registry(), vec!["alpha"]),
            ],
        };
        let g = build(&lock).expect("build");
        let err = detect_cycles(&g).expect_err("should fail");
        match err {
            LockfileError::Cycle(cycles) => {
                assert_eq!(cycles.len(), 1);
                // Rotated to start at lex-smallest member, walked along edges.
                assert_eq!(cycles[0], vec!["alpha@1.0", "beta@1.0"]);
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn detect_cycles_handles_deep_linear_chain_without_stack_overflow() {
        // Build a 5000-node linear chain: pkg_0 -> pkg_1 -> ... -> pkg_4999.
        // Recursive strongconnect could overflow at ~1000 nodes on small stacks.
        const N: usize = 5000;
        let mut packages = Vec::with_capacity(N);
        for i in 0..N {
            let name_str = format!("pkg-{:05}", i);
            let deps = if i + 1 < N {
                vec![DepEdge {
                    name: PackageName::from_str(&format!("pkg-{:05}", i + 1)).unwrap(),
                    extra: vec![],
                    marker: None,
                }]
            } else {
                vec![]
            };
            packages.push(Package {
                name: PackageName::from_str(&name_str).unwrap(),
                version: Version::from_str("1.0").unwrap(),
                source: if i == 0 {
                    Source::FirstParty {
                        kind: FirstPartyKind::Virtual,
                        path: ".".into(),
                    }
                } else {
                    Source::Registry {
                        url: Url::parse("https://pypi.org/simple").unwrap(),
                    }
                },
                dependencies: deps,
                sdist: None,
                wheels: vec![],
                metadata: None,
            });
        }

        let lockfile = Lockfile {
            version: 1,
            revision: 3,
            requires_python: ">=3.11".into(),
            packages,
        };

        let graph = build(&lockfile).expect("graph builds");
        detect_cycles(&graph).expect("no cycles in a linear chain");
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

    use crate::config::{Platform, PythonVersion};
    use crate::platform::marker_env;

    fn linux_x86_platform() -> Platform {
        Platform {
            target: "x86_64-unknown-linux-gnu".into(),
            manylinux: Some("2_17".into()),
            musllinux: None,
            macos_min: None,
        }
    }

    #[test]
    fn reachable_drops_marker_false_edges() {
        let mut lock = Lockfile {
            version: 1,
            revision: 3,
            requires_python: ">=3.10".into(),
            packages: vec![
                pkg("app", "0.1", first_party(), vec![]),
                pkg("typing-extensions", "4.0", registry(), vec![]),
            ],
        };
        // Marker-gated edge: app -> typing-extensions when python < 3.11
        lock.packages[0].dependencies.push(DepEdge {
            name: PackageName::from_str("typing-extensions").unwrap(),
            extra: vec![],
            marker: Some(MarkerTree::from_str("python_version < '3.11'").unwrap()),
        });
        let g = build(&lock).expect("build");

        // Under py3.10: typing-extensions IS reachable
        let env_310 = marker_env(&linux_x86_platform(), PythonVersion(3, 10));
        let r = reachable_from(&g, &g.roots, &env_310, &[]);
        assert_eq!(r.len(), 2);

        // Under py3.12: NOT reachable
        let env_312 = marker_env(&linux_x86_platform(), PythonVersion(3, 12));
        let r = reachable_from(&g, &g.roots, &env_312, &[]);
        assert_eq!(r.len(), 1);
    }

    #[test]
    fn reachable_respects_group_gating() {
        let mut lock = Lockfile {
            version: 1,
            revision: 3,
            requires_python: ">=3.12".into(),
            packages: vec![
                pkg("app", "0.1", first_party(), vec![]),
                pkg("pytest", "8.0", registry(), vec![]),
            ],
        };
        // Test-group-gated edge: app -> pytest when extra == 'test'
        lock.packages[0].dependencies.push(DepEdge {
            name: PackageName::from_str("pytest").unwrap(),
            extra: vec![],
            marker: Some(MarkerTree::from_str("extra == 'test'").unwrap()),
        });
        let g = build(&lock).expect("build");
        let env = marker_env(&linux_x86_platform(), PythonVersion(3, 12));

        // Without include_groups, pytest NOT reachable
        let r = reachable_from(&g, &g.roots, &env, &[]);
        assert_eq!(r.len(), 1);

        // With include_groups = ["test"], pytest IS reachable
        let r = reachable_from(&g, &g.roots, &env, &["test".to_string()]);
        assert_eq!(r.len(), 2);
    }
}
