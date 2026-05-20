# S1 — uv.lock Parser & Dep Graph Spec

**Status:** draft v1 (2026-05-20)
**Companion to:** `2026-05-20-muntjac-design.md` (full design), `2026-05-20-muntjac-roadmap.md` (roadmap), `2026-05-20-muntjac-s0-scaffolding-design.md` (S0 spec)
**Stage:** S1 (second phase-1 stage; depends on S0)

---

## 1. Scope

S1 turns `uv.lock` into a typed, navigable dep graph and exposes it via a hidden `muntjac debug print-deps` JSON command. Everything downstream (S2 wheel selection, S3 BUCK emission) consumes the data this stage produces.

**In:** `uv.lock` + `muntjac.toml` (platforms, python_versions, optional `[lockfile].include_groups`).
**Out:** A `ResolvedGraph` Rust type plus its JSON projection per `(platform, python_version)`.

**Not in S1:**
- Wheel selection (S2 — given platform+python+package, pick the right wheel).
- BUCK emission (S3).
- Sdist handling (S5).
- Network access of any kind (S1 reads the file; doesn't fetch anything).

---

## 2. Crate picks (additions to S0's set)

Add to `[dependencies]`:

- **`pep508_rs`** — PEP 508 marker parser + evaluator. Same crate uv uses internally.
- **`pep440_rs`** — PEP 440 version comparison. Needed for version-comparison markers (e.g. `python_full_version < '3.11'`) and for version-string comparisons in the lockfile.
- **`url`** — URL parsing for package source URLs.

No removals from S0's set. No version bumps.

---

## 3. Type model

Lives in `src/lock/types.rs`.

```rust
use std::collections::BTreeMap;
use pep440_rs::Version;
use pep508_rs::{MarkerTree, PackageName};
use url::Url;
use crate::config::PythonVersion;

/// One row of uv.lock's [[package]] array, normalized.
#[derive(Debug, Clone)]
pub struct Package {
    pub name: PackageName,            // pep508_rs-validated PEP 503 name
    pub version: Version,             // pep440_rs Version
    pub source: Source,
    pub dependencies: Vec<DepEdge>,   // raw edges from [[package]].dependencies
    pub sdist: Option<Sdist>,
    pub wheels: Vec<Wheel>,           // empty for first-party / sdist-only
    pub metadata: Option<Metadata>,   // [[package]].metadata, if present
}

/// Where the package came from. Determines downstream behavior.
#[derive(Debug, Clone)]
pub enum Source {
    Registry { url: Url },            // PyPI or compatible index
    Git { url: Url, rev: String, subdirectory: Option<String> },
    FirstParty { kind: FirstPartyKind, path: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FirstPartyKind { Virtual, Editable, Directory, Path }

/// One entry of [[package]].dependencies. Marker may gate this edge.
#[derive(Debug, Clone)]
pub struct DepEdge {
    pub name: PackageName,
    pub extra: Vec<String>,           // extras requested, e.g. ["parquet"]
    pub marker: Option<MarkerTree>,   // parsed PEP 508 marker; None = always applies
}

#[derive(Debug, Clone)]
pub struct Sdist {
    pub url: Url,
    pub hash: String,                 // "sha256:..."
    pub size: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct Wheel {
    pub url: Url,
    pub hash: String,
    pub size: Option<u64>,
    pub filename: String,             // parsed from URL; PEP 425 tag inspection happens in S2
}

#[derive(Debug, Clone)]
pub struct Metadata {
    pub requires_dist: Vec<DepEdge>,  // [[package.metadata]].requires-dist
    pub provides_extras: Vec<String>,
}

/// Top-level lockfile post-parse, pre-graph-construction.
#[derive(Debug, Clone)]
pub struct Lockfile {
    pub version: u32,                 // must be 1
    pub revision: u32,                // accept >= 3, warn on lower
    pub requires_python: pep508_rs::VersionSpecifiers,
    pub packages: Vec<Package>,
}
```

`PackageName` (from `pep508_rs`) normalizes PEP 503 (`Foo_Bar` → `foo-bar`), so dep edges always resolve correctly regardless of case/separator in the source.

---

## 4. uv.lock parser

Two-layer pattern, mirroring S0's `RawConfig` → `Config`:

- **`RawLockfile`** (private) — `#[derive(Deserialize)]` mirroring the TOML wire format. Forgiving of unknown fields (no `deny_unknown_fields`) so future uv revisions don't break parsing.
- **`Lockfile`** (public, from §3) — the domain type, built via `from_raw` doing name normalization, marker parsing, source-variant disambiguation.

```rust
// src/lock/parser.rs
pub fn parse(toml_src: &str) -> Result<Lockfile, LockfileError> {
    let raw: RawLockfile = toml::from_str(toml_src)?;
    Lockfile::from_raw(raw)
}
```

**Version policy in `from_raw`:**

```rust
if raw.version != 1 {
    return Err(LockfileError::UnsupportedVersion(raw.version));
}
if raw.revision < 3 {
    warn!("uv.lock revision {} is older than the tested floor (3); proceeding but some fields may be missing", raw.revision);
}
```

**Source disambiguation:** `source` in uv.lock is a single inline table with exactly one of `{ registry = "..." }`, `{ git = "...", rev = "...", subdirectory = "..." }`, `{ virtual = "..." }`, `{ editable = "..." }`, `{ directory = "..." }`, `{ path = "..." }`. Parser collapses these into the `Source` enum.

**Marker parsing:** every `marker = "..."` string in `dependencies[]` is parsed via `pep508_rs::MarkerTree::from_str`. Parse errors include the (package, dep) tuple.

**Errors** (`LockfileError` in `src/error.rs`):

```rust
#[derive(Debug, Error)]
pub enum LockfileError {
    #[error("failed to parse uv.lock: {0}")]
    Parse(#[from] toml::de::Error),
    #[error("unsupported uv.lock version `{0}`; muntjac supports version = 1")]
    UnsupportedVersion(u32),
    #[error("package `{package}`: invalid version `{value}`: {reason}")]
    BadVersion { package: String, value: String, reason: String },
    #[error("package `{package}`: source must have exactly one of registry/git/virtual/editable/directory/path; found {found:?}")]
    AmbiguousSource { package: String, found: Vec<&'static str> },
    #[error("package `{package}`: dep `{dep}`: marker `{marker}` failed to parse: {reason}")]
    BadMarker { package: String, dep: String, marker: String, reason: String },
    #[error("package `{source}`: dep `{dep}` has no matching [[package]] entry")]
    UnresolvedDep { source: String, dep: String },
    #[error("duplicate package name `{name}` (found versions {versions:?}); S1 expects one version per name")]
    DuplicatePackageName { name: String, versions: Vec<String> },
    #[error("dependency cycle(s) detected: {0:?}")]
    Cycle(Vec<String>),
}
```

Both `ConfigError` (S0) and `LockfileError` (S1) are re-exported from `src/error.rs` so the CLI layer converts any to `anyhow::Error`.

---

## 5. Marker evaluation

Lives in `src/platform.rs`. Wraps `pep508_rs::MarkerEnvironment`.

```rust
pub fn marker_env(platform: &crate::config::Platform, python: crate::config::PythonVersion) -> MarkerEnvironment {
    let (os_name, sys_platform, platform_system, platform_machine, platform_release) =
        derive_env_strings(&platform.target);

    MarkerEnvironmentBuilder {
        implementation_name: "cpython",
        implementation_version: &full_version(python),
        os_name,
        platform_machine,
        platform_python_implementation: "CPython",
        platform_release,
        platform_system,
        platform_version: "",
        python_full_version: &full_version(python),
        python_version: &short_version(python),
        sys_platform,
    }.into()
}

pub fn marker_matches(marker: Option<&MarkerTree>, env: &MarkerEnvironment) -> bool {
    match marker {
        None => true,
        Some(m) => m.evaluate(env, &mut Vec::new()),
    }
}
```

### Triple → env-string mapping (table-driven for auditability)

| Target triple | `os_name` | `sys_platform` | `platform_system` | `platform_machine` |
|---|---|---|---|---|
| `x86_64-unknown-linux-gnu` | `posix` | `linux` | `Linux` | `x86_64` |
| `aarch64-unknown-linux-gnu` | `posix` | `linux` | `Linux` | `aarch64` |
| `x86_64-unknown-linux-musl` | `posix` | `linux` | `Linux` | `x86_64` |
| `aarch64-unknown-linux-musl` | `posix` | `linux` | `Linux` | `aarch64` |
| `x86_64-apple-darwin` | `posix` | `darwin` | `Darwin` | `x86_64` |
| `aarch64-apple-darwin` | `posix` | `darwin` | `Darwin` | `arm64` |

`platform_release` is left empty (matches uv's default; only exotic markers use it).

`python_full_version` for a `PythonVersion(3, 12)` is `"3.12.0"` — the lowest patch — matching uv's convention.

**musl markers:** PEP 508 has no libc field. musllinux Linux and gnu Linux look identical to marker eval. Users wanting libc-conditional behavior must use muntjac fixup cfg's `target_env = "musl"` (S6's domain). Documented.

**Verification:** a unit-test suite in `src/platform.rs` cross-checks against `pep508_rs`'s own examples plus real-world markers (numpy's `platform_system != 'Emscripten'`, cryptography's `python_version < '3.11'`, etc.).

---

## 6. Dep graph construction & cycle detection

Lives in `src/lock/graph.rs`. Adjacency-map + custom DFS, no `petgraph`.

```rust
pub type NodeId = u32;

#[derive(Debug, Clone)]
pub struct DepGraph {
    pub nodes: Vec<GraphNode>,                              // index = NodeId
    by_name: BTreeMap<PackageName, NodeId>,
    pub roots: Vec<NodeId>,                                 // first-party packages
}

#[derive(Debug, Clone)]
pub struct GraphNode {
    pub pkg: Package,
    pub edges_out: Vec<NodeId>,
    pub edge_markers: Vec<Option<MarkerTree>>,              // parallel to edges_out
    pub edge_extras:  Vec<Vec<String>>,                     // parallel to edges_out
}
```

### Construction algorithm

1. Walk `Lockfile.packages`. Allocate a `NodeId` per package, build a `GraphNode`.
2. Index by `PackageName`. If two packages share a name with different versions, that's a uv lockfile shape we don't support in S1 — error with `LockfileError::DuplicatePackageName`.
3. Second pass: resolve each `DepEdge.name` → `NodeId` via the index. Unresolvable → `LockfileError::UnresolvedDep { source, dep }`.
4. Identify roots: packages with `Source::FirstParty { kind: Virtual | Editable | Directory | Path }`. Typically one (workspace root); can be many in a workspace.

### Cycle detection (Tarjan's SCC)

```rust
pub fn detect_cycles(graph: &DepGraph) -> Result<(), LockfileError> {
    let sccs = tarjan_scc(graph);
    let cycles: Vec<Vec<NodeId>> = sccs
        .into_iter()
        .filter(|scc| scc.len() > 1 || self_loop(graph, scc[0]))
        .collect();
    if cycles.is_empty() { return Ok(()); }
    Err(LockfileError::Cycle(
        cycles.into_iter().map(|scc| {
            scc.into_iter()
                .map(|id| format!("{}@{}", graph.nodes[id as usize].pkg.name, graph.nodes[id as usize].pkg.version))
                .collect::<Vec<_>>()
                .join(" -> ")
        }).collect()
    ))
}
```

Hard error per clarifying decisions. All cycles named, not just the first.

### Reachability per config

```rust
pub fn reachable_from(
    graph: &DepGraph,
    roots: &[NodeId],
    env: &MarkerEnvironment,
    include_groups: &[String],
) -> BTreeSet<NodeId> {
    // DFS from each root, following edges whose markers evaluate true under `env`,
    // and group-gated edges only if the group is in `include_groups`.
    // Returns the reachable set, sorted via BTreeSet ordering.
}
```

---

## 7. Resolved-config projection

Lives in `src/lock/resolved.rs`. Builds the per-(platform, python_version) view consumed by the CLI.

```rust
#[derive(Debug, Clone, serde::Serialize)]
pub struct ResolvedView {
    pub configs: Vec<ResolvedConfig>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ResolvedConfig {
    pub platform: String,                  // e.g. "linux-x86_64-gnu"
    pub python_version: String,            // "3.12"
    pub packages: Vec<ResolvedPackage>,    // sorted by (kind: first-party first, then name)
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ResolvedPackage {
    pub name: String,
    pub version: String,
    pub kind: ResolvedKind,
    #[serde(flatten)]
    pub source_info: SourceInfo,
    pub deps: Vec<String>,                 // "name@version", sorted
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "kebab-case", tag = "kind")]
pub enum ResolvedKind { Registry, Git, FirstParty }

#[derive(Debug, Clone, serde::Serialize)]
#[serde(untagged)]
pub enum SourceInfo {
    Registry { registry: String },
    Git { git: String, rev: String, #[serde(skip_serializing_if = "Option::is_none")] subdirectory: Option<String> },
    FirstParty { first_party_kind: String, first_party_path: String },
}
```

### Projection algorithm

```rust
pub fn project(graph: &DepGraph, cfg: &Config, tree: &Tree) -> ResolvedView {
    let mut configs = Vec::new();
    for (platform_name, platform) in &cfg.platforms {
        for python in &tree.python_versions {
            let env = crate::platform::marker_env(platform, *python);
            let reachable = reachable_from(graph, &graph.roots, &env, &cfg.lockfile.include_groups);
            let packages = reachable.iter()
                .map(|&id| build_resolved_package(graph, id, &env))
                .collect();
            configs.push(ResolvedConfig {
                platform: platform_name.clone(),
                python_version: format!("{}.{}", python.0, python.1),
                packages,
            });
        }
    }
    sort_configs(&mut configs);
    ResolvedView { configs }
}
```

### Determinism

- BTreeMap iteration is sorted.
- Configs sorted `(platform, python_version)` lexicographically.
- Per config: packages sorted by `(kind, name, version)` — first-party first, then alphabetical.
- Per package: `deps` sorted alphabetically.

Same `uv.lock` + same `muntjac.toml` ⇒ byte-identical JSON output. Verified via a fixture.

---

## 8. `muntjac debug print-deps`

Replaces S0's flat-string `Debug` stub with a real `Subcommand` derive.

```rust
// src/cli/debug.rs (rewritten)
#[derive(Subcommand, Debug)]
pub enum DebugOp {
    /// Parse uv.lock and print the resolved dep graph as JSON.
    PrintDeps(PrintDepsArgs),
}

#[derive(Args, Debug)]
pub struct PrintDepsArgs {
    /// Operate on this tree (multi-tree configs).
    #[arg(long)]
    pub tree: Option<String>,

    /// Pretty-print the JSON (default: compact, one line per top-level key).
    #[arg(long)]
    pub pretty: bool,
}
```

The S0 `Debug { subcommand: Option<String>, args: Vec<String> }` shape in `src/cli/mod.rs` is replaced with `Debug { #[command(subcommand)] op: Option<DebugOp> }`. The `--help` snapshot fixture at `tests/snapshots/help__main_help.snap` is refreshed (debug stays hidden from default `--help`, so the snapshot diff should be limited to the hidden `debug print-deps` line appearing under `cargo run -- help debug`).

### Output

```
$ muntjac debug print-deps --pretty
{
  "configs": [
    { "platform": "linux-aarch64-gnu", "python_version": "3.11", "packages": [...] },
    { "platform": "linux-aarch64-gnu", "python_version": "3.12", "packages": [...] },
    { "platform": "linux-x86_64-gnu",  "python_version": "3.11", "packages": [...] },
    ...
  ]
}
```

### Exit codes

- `0` — success, JSON on stdout
- `2` — config error (bad `muntjac.toml`, missing `uv.lock`, etc.); diagnostic on stderr
- `1` — lockfile error (parse, cycle, unresolved dep); diagnostic on stderr

---

## 9. `muntjac.toml` additions

One new optional section, plus a small extension to `Config`.

```toml
# Optional. Default: empty (only base dependencies, no dev/test groups).
[lockfile]
include_groups = ["test"]   # PEP 735 dependency-groups names to include
```

`Config` (defined in S0) gets one new field:

```rust
pub struct Config {
    pub trees: Vec<Tree>,
    pub platforms: BTreeMap<String, Platform>,
    pub fixups: FixupsConfig,
    pub buck: BuckConfig,
    pub lockfile: LockfileConfig,        // NEW in S1
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Default)]
pub struct LockfileConfig {
    #[serde(default)]
    pub include_groups: Vec<String>,
}
```

**Backward compat:** `LockfileConfig` derives `Default`. S0-generated configs (no `[lockfile]` block) still parse. Default = exclude all dependency groups.

**`muntjac init` template update:** add a commented-out hint block:

```toml
# Uncomment to include PEP 735 dependency groups in the resolved graph.
# [lockfile]
# include_groups = ["test"]
```

**`muntjac config check` update:** validates that every name in `include_groups` is a valid identifier (`[a-z][a-z0-9-]*`). Doesn't check the group exists in the lockfile; that's S1's parser's job, surfaced at `print-deps` time.

---

## 10. Module layout (S1 slice)

New files in `src/`:

```
src/
├── lock/
│   ├── mod.rs              # re-exports: pub use parser::parse; pub use types::*; pub use graph::*;
│   ├── types.rs            # §3 — Package, Source, DepEdge, Sdist, Wheel, Metadata, Lockfile
│   ├── parser.rs           # §4 — RawLockfile + from_raw + parse()
│   ├── graph.rs            # §6 — DepGraph, GraphNode, Tarjan SCC, reachable_from
│   └── resolved.rs         # §7 — ResolvedView, project()
├── platform.rs             # §5 — marker_env, marker_matches, triple→env-strings table
└── cli/
    └── debug/
        ├── mod.rs          # §8 — DebugOp enum, dispatcher (replaces flat-string stub)
        └── print_deps.rs   # §8 — print_deps command body
```

Touched-but-not-rewritten existing files:

```
src/lib.rs                  # add `pub mod lock; pub mod platform;`
src/config.rs               # add LockfileConfig + Config.lockfile field (§9)
src/cli/init.rs             # add commented [lockfile] hint to starter template (§9)
src/cli/config_check.rs     # add include_groups identifier validation (§9)
src/cli/mod.rs              # rewrite Debug variant from flat-string to Subcommand (§8)
src/error.rs                # add LockfileError variants (§4)
Cargo.toml                  # add pep508_rs, pep440_rs, url (§2)
tests/snapshots/help__main_help.snap   # refresh after CLI rewrite (§8)
```

The S0 test binaries (`tests/init.rs`, `tests/config_check.rs`, `tests/stubs.rs`, `tests/help.rs`) all must pass unchanged. Any failure that isn't the help snapshot is a regression.

---

## 11. Testing

### Unit tests

- **`src/lock/parser.rs`** — curated `uv.lock` fixtures; assert version, revision, package count, source variants discovered, marker parse success.
- **`src/lock/graph.rs`** — synthetic graphs:
  - Linear chain `a → b → c`
  - Diamond `a → b → d, a → c → d`
  - Self-loop `a → a` ⇒ error
  - 3-cycle `a → b → c → a` ⇒ error with all three named
  - Unresolved dep `a → ghost` ⇒ error naming `ghost`
- **`src/lock/resolved.rs`** — projection determinism; 5-platform × 2-python config shape + sort order.
- **`src/platform.rs`** — marker matrix:
  - `python_version < '3.11'` × (3.10, 3.11, 3.12)
  - `sys_platform == 'linux'` × all platforms
  - `platform_machine == 'arm64'` × macos-arm64 vs macos-x86_64
  - `platform_system != 'Emscripten'`
  - `python_full_version >= '3.10' and python_full_version < '3.13'`
  - 10+ cross-checks against `pep508_rs::MarkerTree::evaluate` directly.

### Fixture tests (`tests/fixtures/lock/`)

```
01-pure-python/                # requests + transitive
02-env-markers/                # typing_extensions; python_version < '3.11'
03-workspace/                  # uv workspace, 2 members + shared third-party
04-extras/                     # pandas[parquet] pulls in pyarrow
05-dev-deps/                   # [dependency-groups] test = [...]; include_groups=["test"]
06-cycle-error/                # hand-crafted cyclic lockfile; expected error message
07-unresolved-dep-error/       # dep with no matching package; expected error
08-multi-platform-marker/      # sys_platform == 'darwin' gated dep
09-determinism/                # run print-deps twice; byte-compare
```

Each scenario contains `pyproject.toml` + `uv.lock` + `muntjac.toml` + either `expected-print-deps.json` (golden) or `expected-error.txt`.

### Integration test (`tests/print_deps.rs`)

`assert_cmd`-driven. Per fixture: invoke `muntjac -C <fixture-dir> debug print-deps --pretty`, capture stdout, byte-compare to golden. Cycle/unresolved-dep fixtures assert non-zero exit + expected stderr substring.

### End-to-end determinism

`runs_twice_identically` test invokes the same fixture twice and `assert_eq!`s stdouts.

---

## 12. Exit criteria

Stage is complete when ALL of:

1. `muntjac debug print-deps` (hidden command) reads `uv.lock` + `muntjac.toml`, prints sorted deterministic JSON. `--pretty` controls formatting; default compact.
2. The 9 fixture tests pass:
   - `01–05`, `08` produce byte-identical output to their golden JSON.
   - `06-cycle-error`, `07-unresolved-dep-error` exit non-zero with the expected error string on stderr.
   - `09-determinism` produces same output across two invocations.
3. Unit tests cover: parser shape validation, graph construction (chain/diamond/cycle/self-loop/unresolved), marker evaluation against `pep508_rs` reference, projection determinism.
4. `Config` gains `lockfile: LockfileConfig` with serde Default; S0-shaped `muntjac.toml` still parses.
5. `muntjac config check` validates `include_groups` identifier shape; rejects e.g. `include_groups = ["bad name"]`.
6. The S0 test suite (29 tests) still passes, plus the new S1 tests. `--help` snapshot is refreshed.
7. `cargo build --locked`, `cargo clippy --all-targets --locked -- -D warnings`, `cargo fmt --all -- --check`, `cargo test --locked` all green on the CI matrix.
8. `LockfileError` variants surface (package, dep, marker) tuples in messages — verified by a snapshot test on one error scenario.

---

## 13. Open questions & risks

- **`pep508_rs` API stability.** Pre-1.0 but actively maintained by Astral. Pin to a specific version range in `Cargo.toml`; bump deliberately.
- **PEP 735 dependency-groups in uv.lock format.** uv represents these via `[[package.metadata]].requires-dist` with `marker = "extra == 'test'"` — or via a separate mechanism, depending on uv version. The `05-dev-deps` fixture exercises this; if the format differs, this section gets a follow-up.
- **`python_full_version` semantics.** `X.Y.0` (lowest patch) is uv's conservative choice; sometimes diverges from user expectation. Documented in the manual; we follow uv's convention.
- **Workspace-with-shared-third-party** (reindeer's `[extern_crates]` analog). Out of scope for S1. `Source::FirstParty` carries enough info that S6/S7 can later add fixup-style external-target substitution.
- **`musllinux` vs `manylinux` markers.** PEP 508 has no libc field. Surfaces in S2 (wheel selector) and S6 (fixup cfg syntax), not S1.

---

## 14. Follow-ups (not S1)

Captured, not committed:

- S2 will consume `ResolvedView` to do PEP 425 wheel selection per (package, platform, python).
- S6 will reuse `MarkerEnvironment` for the fixup `cfg()` predicate evaluator.
- If a `Source::FirstParty { kind: Path }` package appears with declared wheels (rare but theoretically possible), S2 needs a policy. Not blocking S1.
- The `--check` global flag (registered as a no-op in S0) gains meaning in S1 for `print-deps` only when the design wants byte-identical re-invocation gating in CI; currently unused. Defer.
