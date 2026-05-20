# Muntjac S2 — Platform model & wheel selector — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Pick the correct wheel deterministically for every `(package, version, platform, python_version)` cell, using PEP 425's compatible-tag list algorithm, and surface the result via `muntjac debug pick-wheels` JSON output. Fold in the S2-targeted tech-debt items while we're touching the same files.

**Architecture:** Three new modules under `src/wheel/`: `tag.rs` parses PEP 427 wheel filenames (canonicalizing PEP 600/656 aliases at parse time); `compat.rs` builds an ordered `CompatibleTags` list for a given `(Platform, PythonVersion)`, walking the python axis (cp / abi3 / py) crossed with the platform axis (manylinux / musllinux / macOS); `select.rs` picks the best wheel by minimum rank lookup. The selector treats the wheel list as a stable input by sorting filenames before iterating. Strict baseline validation in `Config::validate` makes the existing `manylinux`/`musllinux`/`macos_min` fields load-bearing, replacing inert string storage with structured accessors.

**Tech Stack:** Rust 2024 (edition pinned in `Cargo.toml`), `pep440_rs`, `pep508_rs`, `serde`/`serde_json`, `thiserror`, `insta` for snapshot tests, `assert_cmd` + `predicates` + `tempfile` for integration tests.

---

## File structure

**New files:**

| Path | Responsibility |
|---|---|
| `src/wheel/mod.rs` | Re-exports `Tag`, `WheelTag`, `CompatibleTags`, `PickResult`, `pick_wheel`. |
| `src/wheel/tag.rs` | `Tag`, `PythonTag`, `AbiTag`, `PlatformTag`, `LinuxArch`, `MacArch`, `WheelTag`, `TagParseError`, `parse_filename()`. |
| `src/wheel/compat.rs` | `CompatibleTags` storage + `rank_of()`, plus `build_compatible_tags()`, `build_python_axis()`, `build_platform_axis()`. |
| `src/wheel/select.rs` | `PickResult` enum + `pick_wheel()`. |
| `src/cli/debug/pick_wheels.rs` | `muntjac debug pick-wheels` handler + JSON shape. |
| `tests/fixtures/lock/README.md` | Doc explaining frozen-artifact convention for `tests/fixtures/lock/`. |
| `tests/fixtures/wheel/README.md` | Same convention applied to `tests/fixtures/wheel/`. |
| `tests/fixtures/wheel/01-numpy-matrix/` | Real numpy 2.1.3 wheel list + golden JSON. |
| `tests/fixtures/wheel/02-musllinux-only/` | Package with only `*musllinux*` tags + golden. |
| `tests/fixtures/wheel/03-no-wheel/` | Package with `cp310` wheels only; `NoWheel` outcome + golden. |
| `tests/fixtures/wheel/04-pure-python/` | `py3-none-any` only + golden. |
| `tests/fixtures/wheel/05-determinism/` | Reuses `01-numpy-matrix`; asserts byte-identical output across runs. |
| `tests/pick_wheels.rs` | Integration test runner for the fixtures above. |

**Modified files:**

| Path | Change |
|---|---|
| `src/error.rs` | `LockfileError::Cycle` storage shape (`Vec<String>` → `Vec<Vec<String>>`) + hand-rolled `Display`. |
| `src/lock/graph.rs` | `detect_cycles()` builds per-SCC member lists in edge-walk order; doc-comment expansion on `reachable_with_extras`. |
| `src/config.rs` | Strict baseline validation; `Platform` accessor methods; `include_groups` dedup. |
| `src/platform.rs` | `derive_env_strings` warning branch → `unreachable!()`; `marker_matches` doc comment. |
| `src/cli/mod.rs` | New `Globals::workdir() -> PathBuf` helper; `run()` no longer calls `std::env::set_current_dir`. |
| `src/cli/debug/mod.rs` | New `PickWheels` `DebugOp` variant. |
| `src/cli/debug/print_deps.rs` | Use `Globals::workdir()` instead of `std::env::current_dir()`. |
| `tests/fixtures/lock/06-cycle-error/expected-error.txt` | Tightened to assert a member identifier line. |
| `docs/superpowers/TECH_DEBT.md` | Items moved to `## Resolved` with closing SHAs. |
| `docs/superpowers/specs/2026-05-20-muntjac-roadmap.md` | S2 row marked ✅ shipped. |

---

## Phase 1 — Tech-debt prerequisites

These three tasks land first because they touch files the S2 work will modify heavily; doing them after would force tedious re-edits.

### Task 1: Cycle error reformat

**Files:**
- Modify: `src/error.rs`
- Modify: `src/lock/graph.rs::detect_cycles`
- Modify: `src/lock/graph.rs` tests

The current `LockfileError::Cycle(Vec<String>)` debug-formats as `["a@1.0 -> b@1.0"]` (Rust syntax leaks). We change storage to `Vec<Vec<String>>` (outer = cycles, inner = members in walk order), then hand-roll `Display`. `detect_cycles()` walks the SCC subgraph from the lex-smallest member, preferring unvisited outgoing edges.

- [ ] **Step 1: Update test in `src/lock/graph.rs::tests` to reflect new storage**

```rust
#[test]
fn detect_cycles_finds_simple_cycle() {
    // alpha -> beta -> alpha
    let graph = build_test_graph(&[
        ("alpha", "1.0", &["beta@1.0"]),
        ("beta", "1.0", &["alpha@1.0"]),
    ]);
    let cycles = graph.detect_cycles();
    assert_eq!(cycles.len(), 1);
    // Rotated to start at lex-smallest member, walked along edges.
    assert_eq!(cycles[0], vec!["alpha@1.0", "beta@1.0"]);
}
```

- [ ] **Step 2: Run test — verify it fails**

Run: `cargo test --lib lock::graph::tests::detect_cycles_finds_simple_cycle`
Expected: FAIL — `cycles[0]` is currently a `String`, not a `Vec<String>`.

- [ ] **Step 3: Change `LockfileError::Cycle` storage in `src/error.rs`**

```rust
#[derive(Debug, thiserror::Error)]
pub enum LockfileError {
    // ... other variants unchanged ...

    Cycle(Vec<Vec<String>>),

    // ... other variants unchanged ...
}

// Hand-rolled Display for the Cycle variant (override #[error("...")] entirely).
impl std::fmt::Display for LockfileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LockfileError::Cycle(cycles) => {
                writeln!(f, "dependency cycle(s) detected:")?;
                for cycle in cycles {
                    // Display each cycle as: "  - a@1 -> b@1 -> a@1"
                    write!(f, "  - ")?;
                    for (i, node) in cycle.iter().enumerate() {
                        if i > 0 { write!(f, " -> ")?; }
                        write!(f, "{node}")?;
                    }
                    // Close the cycle visually by repeating the first node.
                    if let Some(first) = cycle.first() {
                        writeln!(f, " -> {first}")?;
                    } else {
                        writeln!(f)?;
                    }
                }
                Ok(())
            }
            // Delegate other variants to thiserror's generated Display.
            // We do this by using the generated impl name — but since we can't
            // call into thiserror's impl directly, fall back to per-variant arms.
            // (Other variants kept compact for brevity; spell out as in existing src/error.rs.)
            other => write!(f, "{}", other.thiserror_display()),
        }
    }
}
```

Actually the cleanest pattern, since we want hand-rolled Display only for Cycle: remove `#[derive(thiserror::Error)]` and write Display + std::error::Error manually for the whole enum. That's a lot of boilerplate. The shorter path: keep `#[derive(thiserror::Error)]` and put the multi-line format in the `#[error("...")]` attribute using a custom format function.

Use thiserror's `#[error(fmt = path::to::fn)]` (thiserror 2.0 supports this):

```rust
#[derive(Debug, thiserror::Error)]
pub enum LockfileError {
    // ... other variants unchanged ...

    #[error(fmt = fmt_cycle)]
    Cycle(Vec<Vec<String>>),

    // ... other variants unchanged ...
}

fn fmt_cycle(cycles: &Vec<Vec<String>>, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    writeln!(f, "dependency cycle(s) detected:")?;
    for cycle in cycles {
        write!(f, "  - ")?;
        for (i, node) in cycle.iter().enumerate() {
            if i > 0 { write!(f, " -> ")?; }
            write!(f, "{node}")?;
        }
        if let Some(first) = cycle.first() {
            writeln!(f, " -> {first}")?;
        } else {
            writeln!(f)?;
        }
    }
    Ok(())
}
```

If thiserror 2.0 in our Cargo.lock doesn't support `#[error(fmt = ...)]` (a feature flag may be required, or the syntax may differ), the fallback is to drop `#[derive(thiserror::Error)]` for this enum and implement Display + Error manually. Verify with `cargo build` before the next step; if it doesn't compile, fall back to manual Display + an `impl std::error::Error for LockfileError {}` empty impl (the manual `Error` impl needs `source()` only if any variant wraps another error — verify against current variants).

- [ ] **Step 4: Update `detect_cycles` in `src/lock/graph.rs` to walk SCCs in edge order**

```rust
pub fn detect_cycles(&self) -> Vec<Vec<String>> {
    // Run Tarjan SCC; for each SCC with > 1 node OR with a self-loop, emit a cycle.
    let sccs = self.tarjan_sccs();  // returns Vec<Vec<NodeId>>; existing function

    let mut out: Vec<Vec<String>> = Vec::new();
    for scc in sccs {
        let is_cycle = scc.len() > 1 || self.has_self_loop(scc[0]);
        if !is_cycle { continue; }

        // Find the lex-smallest member by display string.
        let mut members: Vec<NodeId> = scc.clone();
        members.sort_by_key(|n| self.display_string(*n));
        let start = members[0];

        // Walk along outgoing edges, preferring unvisited successors that are
        // also in the SCC. Stop when we'd revisit any node.
        let scc_set: std::collections::HashSet<NodeId> = scc.iter().copied().collect();
        let mut path: Vec<NodeId> = vec![start];
        let mut visited: std::collections::HashSet<NodeId> = [start].into_iter().collect();

        loop {
            let current = *path.last().unwrap();
            let next = self.nodes[current.0]
                .edges_out
                .iter()
                .filter(|&&succ| scc_set.contains(&succ))
                .find(|&&succ| !visited.contains(&succ))
                .or_else(|| {
                    // No unvisited successor in SCC; the cycle closes here.
                    self.nodes[current.0]
                        .edges_out
                        .iter()
                        .find(|&&succ| scc_set.contains(&succ))
                });
            match next {
                Some(&n) if !visited.contains(&n) => {
                    path.push(n);
                    visited.insert(n);
                }
                Some(_) | None => break,
            }
        }

        let path_strs: Vec<String> = path.into_iter().map(|n| self.display_string(n)).collect();
        out.push(path_strs);
    }
    out.sort();  // deterministic order across runs
    out
}
```

(`tarjan_sccs`, `has_self_loop`, and `display_string` are existing helpers in `graph.rs` — if any are missing in the current code, extract them from existing `detect_cycles` logic. `display_string(n)` returns `"<pkg>@<version>"`.)

- [ ] **Step 5: Update existing callers in graph.rs**

The existing `detect_cycles` already returns `Vec<String>`; the callsite that wraps the result into `LockfileError::Cycle(...)` lives in `Lockfile::from_raw` or `DepGraph::build` (whichever currently calls it). Update that callsite to pass the new `Vec<Vec<String>>` directly.

- [ ] **Step 6: Run all tests — confirm cycle test passes and nothing else broke**

Run: `cargo test --lib`
Expected: all S1 tests still pass; the updated cycle test now passes.

- [ ] **Step 7: Commit**

```bash
git add src/error.rs src/lock/graph.rs
git commit -m "fix(s2): cycle error renders as multi-line bulleted output

LockfileError::Cycle now stores Vec<Vec<String>> (outer = cycles,
inner = members in actual edge-walk order). Display formats as:

  dependency cycle(s) detected:
    - alpha@1.0 -> beta@1.0 -> alpha@1.0

detect_cycles walks each SCC starting from the lex-smallest member,
preferring unvisited outgoing edges. Output is sorted for determinism.

Closes TECH_DEBT item: 'Cycle error formatting is opaque'."
```

---

### Task 2: Fixture 06 assertion tightening

**Files:**
- Modify: `tests/fixtures/lock/06-cycle-error/expected-error.txt`

The current fixture asserts only the substring `dependency cycle(s) detected`. Tighten by adding a member identifier so a regression dropping cycle members from the output is caught.

- [ ] **Step 1: Inspect the current fixture**

Run: `cat tests/fixtures/lock/06-cycle-error/expected-error.txt`
Expected: file contains only `dependency cycle(s) detected`.

Also read `tests/fixtures/lock/06-cycle-error/uv.lock` to identify what cycle's members would appear in the output. Use `cargo run -- -C tests/fixtures/lock/06-cycle-error debug print-deps 2>&1` to see the current error message format with the changes from Task 1.

- [ ] **Step 2: Update expected-error.txt to include a member identifier line**

Replace contents with:

```
dependency cycle(s) detected:
  - alpha@1.0 -> beta@1.0
```

(Substitute the actual member names from `uv.lock`.) The `assert_error` helper in `tests/print_deps.rs` does substring matching on each newline-delimited line, so trailing/leading whitespace on the bullet line is tolerated. If the fixture has multiple cycles, include one bullet line; the test still passes if more bullets follow.

- [ ] **Step 3: Run integration test**

Run: `cargo test --test print_deps -- fixture_06`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add tests/fixtures/lock/06-cycle-error/expected-error.txt
git commit -m "test(s2): fixture 06 asserts cycle member identifier

Tightens the cycle-error fixture so regressions dropping member
names from the output are caught by CI.

Closes TECH_DEBT item: 'Fixture 06-cycle-error assertion is too loose'."
```

---

### Task 3: `Globals::workdir()` plumbing

**Files:**
- Modify: `src/cli/mod.rs`
- Modify: `src/cli/debug/print_deps.rs`
- Modify: `tests/print_deps.rs`

The existing `cli::run()` calls `std::env::set_current_dir(path)` based on the `-C` global, then subcommand handlers read from `std::env::current_dir()`. Replace with an explicit `Globals::workdir() -> PathBuf` that handlers consume directly.

- [ ] **Step 1: Write the failing integration test**

In `tests/print_deps.rs`, add:

```rust
#[test]
fn print_deps_does_not_depend_on_process_cwd() {
    // Run from a temp directory; verify -C still works.
    let tmp = tempfile::tempdir().unwrap();
    let fixture = std::fs::canonicalize("tests/fixtures/lock/01-pure-python").unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_muntjac"))
        .current_dir(tmp.path())            // process cwd is unrelated to fixture
        .arg("-C").arg(&fixture)
        .args(["debug", "print-deps"])
        .output()
        .expect("run muntjac");
    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
}
```

- [ ] **Step 2: Run — verify it passes with the current (chdir-based) implementation**

Run: `cargo test --test print_deps print_deps_does_not_depend_on_process_cwd`
Expected: PASS (the current implementation handles this via `set_current_dir`, but the test pins the behavior so refactoring can't regress it).

- [ ] **Step 3: Add `Globals::workdir()` accessor in `src/cli/mod.rs`**

```rust
impl Globals {
    /// Returns the resolved working directory: either the value of `-C` (canonicalized)
    /// or `std::env::current_dir()` if not set.
    pub fn workdir(&self) -> std::io::Result<std::path::PathBuf> {
        match &self.cd {
            Some(p) => std::fs::canonicalize(p),
            None => std::env::current_dir(),
        }
    }
}
```

- [ ] **Step 4: Update `src/cli/debug/print_deps.rs` to use `globals.workdir()`**

Find the existing call (probably `std::env::current_dir().unwrap()` near the top of the handler) and replace:

```rust
pub fn run(globals: &Globals, args: &PrintDepsArgs) -> Result<(), MuntjacError> {
    let workdir = globals.workdir().map_err(MuntjacError::Io)?;
    let config_path = workdir.join("muntjac.toml");
    // ... rest unchanged, using `workdir` and `config_path` ...
}
```

- [ ] **Step 5: Remove `std::env::set_current_dir(path)` from `cli::run()`**

In `src/cli/mod.rs`, find the block that does:

```rust
if let Some(path) = &globals.cd {
    std::env::set_current_dir(path)
        .map_err(|e| ...)?;
}
```

Delete it. Handlers now read `globals.workdir()` themselves.

- [ ] **Step 6: Run all tests**

Run: `cargo test`
Expected: all tests pass, including the new `print_deps_does_not_depend_on_process_cwd`.

- [ ] **Step 7: Commit**

```bash
git add src/cli/mod.rs src/cli/debug/print_deps.rs tests/print_deps.rs
git commit -m "refactor(s2): plumb Globals::workdir() instead of chdir

Replaces the implicit std::env::set_current_dir(path) in cli::run with
an explicit Globals::workdir() helper that each handler consumes. New
integration test pins the contract that subcommands work without
process cwd changes.

Closes TECH_DEBT item: 'print_deps.rs uses std::env::current_dir()'."
```

---

## Phase 2 — Platform baseline parsing & validation

### Task 4: Strict baseline validation in `Config::validate`

**Files:**
- Modify: `src/config.rs` (`Config::validate` + new helpers)
- Modify: `src/error.rs` (extended `BadPlatform` reasons)
- Modify: `src/config.rs::tests` (new tests for each validation rule)

Each `Platform` must declare the right baseline for its target triple. Validation rules from spec §8.

- [ ] **Step 1: Write the failing test**

In `src/config.rs::tests`:

```rust
#[test]
fn rejects_linux_platform_without_any_baseline() {
    let toml_str = r#"
manifest_path   = "../pyproject.toml"
third_party_dir = "."
python_versions = ["3.12"]

[platforms.linux-bare]
target = "x86_64-unknown-linux-gnu"
"#;
    let config = Config::from_str(toml_str).expect("parse");
    let err = config.validate().expect_err("should fail");
    assert!(matches!(err, crate::error::ConfigError::BadPlatform { ref reason, .. }
        if reason.contains("manylinux") && reason.contains("musllinux")));
}

#[test]
fn rejects_macos_platform_without_macos_min() {
    let toml_str = r#"
manifest_path   = "../pyproject.toml"
third_party_dir = "."
python_versions = ["3.12"]

[platforms.macos-bare]
target = "aarch64-apple-darwin"
"#;
    let config = Config::from_str(toml_str).expect("parse");
    let err = config.validate().expect_err("should fail");
    assert!(matches!(err, crate::error::ConfigError::BadPlatform { ref reason, .. }
        if reason.contains("macos_min")));
}

#[test]
fn rejects_bad_baseline_shape() {
    let toml_str = r#"
manifest_path   = "../pyproject.toml"
third_party_dir = "."
python_versions = ["3.12"]

[platforms.linux]
target = "x86_64-unknown-linux-gnu"
manylinux = "2014"
"#;
    let config = Config::from_str(toml_str).expect("parse");
    let err = config.validate().expect_err("should fail");
    assert!(matches!(err, crate::error::ConfigError::BadPlatform { ref reason, .. }
        if reason.contains("2014") && reason.contains("2_17")));
}

#[test]
fn rejects_musllinux_on_gnu_target() {
    let toml_str = r#"
manifest_path   = "../pyproject.toml"
third_party_dir = "."
python_versions = ["3.12"]

[platforms.confused]
target = "x86_64-unknown-linux-gnu"
musllinux = "1_2"
"#;
    let config = Config::from_str(toml_str).expect("parse");
    let err = config.validate().expect_err("should fail");
    assert!(matches!(err, crate::error::ConfigError::BadPlatform { ref reason, .. }
        if reason.contains("musllinux") && reason.contains("linux-gnu")));
}
```

- [ ] **Step 2: Run — verify all four fail**

Run: `cargo test --lib config::tests::rejects_linux_platform_without_any_baseline config::tests::rejects_macos_platform_without_macos_min config::tests::rejects_bad_baseline_shape config::tests::rejects_musllinux_on_gnu_target`
Expected: all four FAIL (validation currently accepts these).

- [ ] **Step 3: Implement the new validation rules in `src/config.rs`**

Add a helper `validate_platform_baseline` and call it from `Config::validate`:

```rust
fn validate_platform_baseline(name: &str, p: &Platform) -> Result<(), crate::error::ConfigError> {
    let is_gnu  = p.target.ends_with("linux-gnu");
    let is_musl = p.target.ends_with("linux-musl");
    let is_mac  = p.target.ends_with("apple-darwin");

    // Shape checks — fail fast on malformed strings before policy checks.
    if let Some(s) = &p.manylinux {
        parse_underscore_pair(s).map_err(|_| crate::error::ConfigError::BadPlatform {
            name: name.into(),
            reason: format!("manylinux baseline must be N_M (got `{s}`); use `2_17` not `2014`"),
        })?;
    }
    if let Some(s) = &p.musllinux {
        parse_underscore_pair(s).map_err(|_| crate::error::ConfigError::BadPlatform {
            name: name.into(),
            reason: format!("musllinux baseline must be N_M (got `{s}`)"),
        })?;
    }
    if let Some(s) = &p.macos_min {
        parse_dot_pair(s).map_err(|_| crate::error::ConfigError::BadPlatform {
            name: name.into(),
            reason: format!("macos_min must be MAJOR.MINOR (got `{s}`)"),
        })?;
    }

    // Policy: presence + libc coherence.
    if is_gnu || is_musl {
        if p.manylinux.is_none() && p.musllinux.is_none() {
            return Err(crate::error::ConfigError::BadPlatform {
                name: name.into(),
                reason: "linux platform must declare manylinux and/or musllinux baseline".into(),
            });
        }
        if is_gnu && p.musllinux.is_some() {
            return Err(crate::error::ConfigError::BadPlatform {
                name: name.into(),
                reason: "musllinux baseline only valid on *-linux-musl targets, not linux-gnu".into(),
            });
        }
        if is_musl && p.manylinux.is_some() {
            return Err(crate::error::ConfigError::BadPlatform {
                name: name.into(),
                reason: "manylinux baseline only valid on *-linux-gnu targets, not linux-musl".into(),
            });
        }
    }
    if is_mac && p.macos_min.is_none() {
        return Err(crate::error::ConfigError::BadPlatform {
            name: name.into(),
            reason: "macOS platform must declare macos_min (deployment target)".into(),
        });
    }
    Ok(())
}

fn parse_underscore_pair(s: &str) -> Result<(u32, u32), ()> {
    let (a, b) = s.split_once('_').ok_or(())?;
    Ok((a.parse().map_err(|_| ())?, b.parse().map_err(|_| ())?))
}

fn parse_dot_pair(s: &str) -> Result<(u32, u32), ()> {
    let (a, b) = s.split_once('.').ok_or(())?;
    Ok((a.parse().map_err(|_| ())?, b.parse().map_err(|_| ())?))
}
```

Then in `Config::validate`:

```rust
pub fn validate(&self) -> Result<(), crate::error::ConfigError> {
    for (name, platform) in &self.platforms {
        validate_target_triple(name, &platform.target)?;
        validate_platform_baseline(name, platform)?;
    }
    validate_registry(&self.fixups.registry)?;
    for g in &self.lockfile.include_groups {
        validate_group_name(g)?;
    }
    Ok(())
}
```

**Wire validation into `Config::from_str`** so the "no code path sees an unvalidated `Platform`" invariant from spec §8 holds:

```rust
impl FromStr for Config {
    type Err = crate::error::ConfigError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let raw: RawConfig = toml::from_str(s).map_err(crate::error::ConfigError::Parse)?;
        let config = Self::from_raw(raw)?;
        config.validate()?;
        Ok(config)
    }
}
```

This is a behavior change for callers of `from_str` — invalid configs that previously parsed and only failed at `.validate()` time now fail at `from_str` directly. Tests that explicitly call `.validate()` separately keep working (the second call is idempotent).

- [ ] **Step 4: Update existing test fixtures**

Multiple test fixtures in `src/config.rs::tests` use linux platforms without baselines. Find each and add `manylinux = "2_17"`. Find each macOS literal and add `macos_min = "11.0"`. Run `cargo test --lib config::tests` and fix every test that breaks until they all pass.

Look out for: the `RawConfig` parsed in `validates_registry_form`, `rejects_bad_python_version`, `rejects_missing_manifest_path_when_no_trees`, `parses_lockfile_config_section`, `lockfile_config_defaults_to_empty`, `rejects_invalid_group_identifier` — many of these omit baseline fields.

Also update `src/cli/init.rs`'s starter template if it currently omits baselines.

Also update the existing fixtures under `tests/fixtures/lock/0*/muntjac.toml` to include baselines (most likely already have them, since they came from real templates; verify).

- [ ] **Step 5: Run — all four new tests pass and existing tests still pass**

Run: `cargo test`
Expected: all tests green.

- [ ] **Step 6: Commit**

```bash
git add src/config.rs src/cli/init.rs tests/fixtures/lock/
git commit -m "feat(s2): strict baseline validation for platforms

Config::validate now enforces:
- Linux platforms must declare manylinux and/or musllinux
- macOS platforms must declare macos_min
- Baseline shapes are validated (N_M for many/musllinux, X.Y for macos_min)
- libc coherence: musllinux only on -linux-musl, manylinux only on -linux-gnu

These were previously inert Option<String> fields; now they're load-bearing."
```

---

### Task 5: Platform baseline accessor methods

**Files:**
- Modify: `src/config.rs` (impl Platform)

After validation succeeds, code that consumes `Platform` (specifically `build_compatible_tags` in Task 11) needs structured access to the parsed baselines.

- [ ] **Step 1: Write the failing test**

In `src/config.rs::tests`:

```rust
#[test]
fn platform_baseline_accessors() {
    let p_linux = Platform {
        target: "x86_64-unknown-linux-gnu".into(),
        manylinux: Some("2_28".into()),
        musllinux: None,
        macos_min: None,
    };
    assert_eq!(p_linux.manylinux_baseline(), Some((2, 28)));
    assert_eq!(p_linux.musllinux_baseline(), None);
    assert_eq!(p_linux.macos_min_parsed(), None);

    let p_mac = Platform {
        target: "aarch64-apple-darwin".into(),
        manylinux: None,
        musllinux: None,
        macos_min: Some("11.0".into()),
    };
    assert_eq!(p_mac.macos_min_parsed(), Some((11, 0)));
}
```

- [ ] **Step 2: Run — verify fails**

Run: `cargo test --lib config::tests::platform_baseline_accessors`
Expected: FAIL — methods don't exist.

- [ ] **Step 3: Implement the accessors in `src/config.rs`**

```rust
impl Platform {
    /// Returns the parsed manylinux baseline `(major, minor)` if set.
    /// Assumes `Config::validate` has run (panics if the string is malformed).
    pub fn manylinux_baseline(&self) -> Option<(u32, u32)> {
        self.manylinux.as_deref().map(|s| parse_underscore_pair(s)
            .expect("validated manylinux string"))
    }

    /// Returns the parsed musllinux baseline `(major, minor)` if set.
    pub fn musllinux_baseline(&self) -> Option<(u32, u32)> {
        self.musllinux.as_deref().map(|s| parse_underscore_pair(s)
            .expect("validated musllinux string"))
    }

    /// Returns the parsed `macos_min` deployment target `(major, minor)` if set.
    pub fn macos_min_parsed(&self) -> Option<(u32, u32)> {
        self.macos_min.as_deref().map(|s| parse_dot_pair(s)
            .expect("validated macos_min string"))
    }
}
```

These re-parse the validated strings every call. Parsing two integers is sub-microsecond; not worth caching.

- [ ] **Step 4: Run — passes**

Run: `cargo test --lib config::tests::platform_baseline_accessors`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/config.rs
git commit -m "feat(s2): Platform baseline accessor methods

manylinux_baseline(), musllinux_baseline(), macos_min_parsed() expose
the validated baselines as Option<(u32, u32)> for downstream consumers
(the wheel selector). Methods assume Config::validate has run."
```

---

### Task 6: `derive_env_strings` unreachable cleanup

**Files:**
- Modify: `src/platform.rs`

Now that strict validation guarantees only known triples reach the function, replace the warning branch with `unreachable!()`.

- [ ] **Step 1: Update the warning branch in `derive_env_strings`**

In `src/platform.rs`, change:

```rust
other => {
    eprintln!("warning: unknown target triple `{other}`; marker eval may be inaccurate");
    EnvStrings {
        os_name: "",
        sys_platform: "",
        platform_system: "",
        platform_machine: "",
    }
}
```

to:

```rust
other => unreachable!(
    "Config::validate must reject unknown target triple `{other}` before reaching here"
),
```

- [ ] **Step 2: Run — all tests pass**

Run: `cargo test`
Expected: PASS — no test ever passed an unvalidated triple to this function.

- [ ] **Step 3: Commit**

```bash
git add src/platform.rs
git commit -m "refactor(s2): derive_env_strings warning branch becomes unreachable

Config::validate's target-triple allowlist guarantees only known
triples reach derive_env_strings. Replace the silent-fallback warning
branch with unreachable!() so the invariant is enforced at runtime
rather than papered over.

Closes TECH_DEBT item: 'derive_env_strings warning branch is unreachable'."
```

---

## Phase 3 — Wheel tag parsing

### Task 7: Tag types

**Files:**
- Create: `src/wheel/mod.rs`
- Create: `src/wheel/tag.rs`
- Modify: `src/main.rs` or `src/lib.rs` (declare `pub mod wheel;`)

Define the type model from spec §4. No parsing logic in this task — just the data shapes plus their `PartialEq`/`Hash` derivations.

- [ ] **Step 1: Write the failing test for type construction**

Create `src/wheel/tag.rs` with the file scaffold; add the test:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tag_equality_and_hash() {
        let a = Tag {
            python: PythonTag::CPython(3, 12),
            abi:    AbiTag::CPython(3, 12),
            plat:   PlatformTag::ManyLinux { major: 2, minor: 17, arch: LinuxArch::X86_64 },
        };
        let b = a.clone();
        assert_eq!(a, b);

        let mut set = std::collections::HashSet::new();
        set.insert(a);
        assert!(set.contains(&b));
    }

    #[test]
    fn arch_variants_are_distinct() {
        assert_ne!(LinuxArch::X86_64, LinuxArch::Aarch64);
        assert_ne!(MacArch::X86_64, MacArch::Arm64);
        assert_ne!(MacArch::Arm64, MacArch::Universal2);
    }
}
```

- [ ] **Step 2: Run — verify fails (file doesn't compile)**

Run: `cargo test --lib wheel::tag`
Expected: FAIL — `src/wheel/tag.rs` doesn't exist.

- [ ] **Step 3: Create the file scaffold**

Create `src/wheel/mod.rs`:

```rust
//! Wheel filename parsing and selection.
//!
//! See `docs/superpowers/specs/2026-05-20-muntjac-s2-wheel-selector-design.md`
//! for the design.

pub mod tag;
// `compat` and `select` modules added in later tasks.

pub use tag::{
    AbiTag, LinuxArch, MacArch, PlatformTag, PythonTag, Tag, TagParseError, WheelTag,
};
```

Create `src/wheel/tag.rs`:

```rust
//! PEP 425/600/656 wheel tag types and filename parser.

/// A single fully-expanded PEP 425 tag triple.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Tag {
    pub python: PythonTag,
    pub abi: AbiTag,
    pub plat: PlatformTag,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PythonTag {
    /// e.g. cp312 → CPython(3, 12)
    CPython(u8, u8),
    /// `py3` → Py(3, None); `py37` → Py(3, Some(7)); `py2` → Py(2, None).
    Py(u8, Option<u8>),
    /// pp310, jy27, ip3 — unsupported, kept opaque.
    Other(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum AbiTag {
    CPython(u8, u8),
    Abi3,
    None,
    Other(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PlatformTag {
    Any,
    ManyLinux { major: u32, minor: u32, arch: LinuxArch },
    MuslLinux { major: u32, minor: u32, arch: LinuxArch },
    MacOs    { major: u32, minor: u32, arch: MacArch },
    Other(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum LinuxArch { X86_64, Aarch64 }

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum MacArch { X86_64, Arm64, Universal2 }

/// All tags expanded from a wheel filename.
#[derive(Debug, Clone)]
pub struct WheelTag {
    pub tags: Vec<Tag>,
    pub raw_filename: String,
}

#[derive(Debug, thiserror::Error)]
pub enum TagParseError {
    #[error("wheel filename `{0}` does not match PEP 427 structure")]
    MalformedFilename(String),
    #[error("unknown structure in tag segment of `{0}`")]
    UnknownTagShape(String),
}

#[cfg(test)]
mod tests {
    // (test block from Step 1)
}
```

Declare the module in the crate root. Check `src/main.rs` first; if it has `mod cli; mod config; ...`, add `mod wheel;` alongside. If the crate has `src/lib.rs`, declare `pub mod wheel;` there.

- [ ] **Step 4: Run — passes**

Run: `cargo test --lib wheel::tag`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/wheel/ src/main.rs
git commit -m "feat(s2): wheel tag type model

Defines Tag, PythonTag, AbiTag, PlatformTag, LinuxArch, MacArch,
WheelTag, TagParseError. No parsing logic yet — just the shapes
the parser (next task) and the compatible-tag builder will use.

PlatformTag carries parsed numeric structure so ordering between
manylinux_2_17 and manylinux_2_28 is type-level, not string compare."
```

---

### Task 8: `parse_filename` with alias canonicalization

**Files:**
- Modify: `src/wheel/tag.rs` (add parser + tests)

PEP 427 filename shape: `{distribution}-{version}(-{build_tag})?-{python}-{abi}-{plat}.whl`. Tag segments may each be compressed (dot-separated sets); expand into the cross product.

Alias canonicalization at parse time:
- `manylinux1_<arch>` → `manylinux_2_5_<arch>`
- `manylinux2010_<arch>` → `manylinux_2_12_<arch>`
- `manylinux2014_<arch>` → `manylinux_2_17_<arch>`

Duplicates removed after expansion (the canonicalization can collide).

- [ ] **Step 1: Write the failing test suite**

Append to `src/wheel/tag.rs::tests`:

```rust
fn parse(s: &str) -> WheelTag {
    parse_filename(s).unwrap_or_else(|e| panic!("parse `{s}`: {e}"))
}

#[test]
fn parses_simple_cp312_manylinux() {
    let w = parse("numpy-2.1.3-cp312-cp312-manylinux_2_17_x86_64.whl");
    assert_eq!(w.tags.len(), 1);
    assert_eq!(w.tags[0], Tag {
        python: PythonTag::CPython(3, 12),
        abi:    AbiTag::CPython(3, 12),
        plat:   PlatformTag::ManyLinux { major: 2, minor: 17, arch: LinuxArch::X86_64 },
    });
}

#[test]
fn parses_pure_python_any() {
    let w = parse("requests-2.32.3-py3-none-any.whl");
    assert_eq!(w.tags.len(), 1);
    assert_eq!(w.tags[0], Tag {
        python: PythonTag::Py(3, None),
        abi:    AbiTag::None,
        plat:   PlatformTag::Any,
    });
}

#[test]
fn parses_abi3_wheel() {
    let w = parse("cryptography-42.0.0-cp37-abi3-manylinux_2_28_x86_64.whl");
    assert_eq!(w.tags[0].abi, AbiTag::Abi3);
}

#[test]
fn expands_compressed_python_tag() {
    let w = parse("foo-1.0-py2.py3-none-any.whl");
    assert_eq!(w.tags.len(), 2);
    assert!(w.tags.contains(&Tag {
        python: PythonTag::Py(2, None), abi: AbiTag::None, plat: PlatformTag::Any
    }));
    assert!(w.tags.contains(&Tag {
        python: PythonTag::Py(3, None), abi: AbiTag::None, plat: PlatformTag::Any
    }));
}

#[test]
fn canonicalizes_manylinux2014_alias() {
    let w = parse("foo-1.0-cp312-cp312-manylinux2014_x86_64.whl");
    assert_eq!(w.tags[0].plat,
        PlatformTag::ManyLinux { major: 2, minor: 17, arch: LinuxArch::X86_64 });
}

#[test]
fn dedupes_after_alias_canonicalization() {
    // Both tags collapse to the same canonical form.
    let w = parse("foo-1.0-cp312-cp312-manylinux_2_17_x86_64.manylinux2014_x86_64.whl");
    assert_eq!(w.tags.len(), 1);
}

#[test]
fn parses_macos_universal2() {
    let w = parse("foo-1.0-cp312-cp312-macosx_11_0_universal2.whl");
    assert_eq!(w.tags[0].plat,
        PlatformTag::MacOs { major: 11, minor: 0, arch: MacArch::Universal2 });
}

#[test]
fn parses_musllinux() {
    let w = parse("foo-1.0-cp312-cp312-musllinux_1_2_x86_64.whl");
    assert_eq!(w.tags[0].plat,
        PlatformTag::MuslLinux { major: 1, minor: 2, arch: LinuxArch::X86_64 });
}

#[test]
fn pypy_tag_classified_as_other() {
    let w = parse("foo-1.0-pp310-pypy310_pp73-manylinux_2_17_x86_64.whl");
    assert!(matches!(w.tags[0].python, PythonTag::Other(ref s) if s == "pp310"));
}

#[test]
fn build_tag_is_ignored() {
    // build-tag (after version, before python tag) is optional and informational.
    let w = parse("foo-1.0-1-cp312-cp312-manylinux_2_17_x86_64.whl");
    assert_eq!(w.tags[0].python, PythonTag::CPython(3, 12));
}

#[test]
fn rejects_missing_whl_suffix() {
    assert!(matches!(parse_filename("foo-1.0-py3-none-any.zip"),
        Err(TagParseError::MalformedFilename(_))));
}

#[test]
fn rejects_too_few_segments() {
    assert!(matches!(parse_filename("foo-1.0.whl"),
        Err(TagParseError::MalformedFilename(_))));
}
```

- [ ] **Step 2: Run — verify all fail**

Run: `cargo test --lib wheel::tag`
Expected: many FAILs — `parse_filename` doesn't exist yet.

- [ ] **Step 3: Implement `parse_filename`**

Add to `src/wheel/tag.rs`:

```rust
/// Parse a PEP 427 wheel filename into all its compatible Tag triples.
pub fn parse_filename(filename: &str) -> Result<WheelTag, TagParseError> {
    let stem = filename
        .strip_suffix(".whl")
        .ok_or_else(|| TagParseError::MalformedFilename(filename.to_string()))?;

    // PEP 427: name-version(-build)?-py-abi-plat
    let parts: Vec<&str> = stem.split('-').collect();
    // Minimum 5 parts (no build tag) or 6 parts (with build tag).
    let (py_seg, abi_seg, plat_seg) = match parts.len() {
        n if n < 5 => return Err(TagParseError::MalformedFilename(filename.to_string())),
        5 => (parts[2], parts[3], parts[4]),
        _ => {
            // Last three segments are always py, abi, plat.
            let n = parts.len();
            (parts[n - 3], parts[n - 2], parts[n - 1])
        }
    };

    let pythons = py_seg.split('.').map(parse_python_tag).collect::<Result<Vec<_>, _>>()?;
    let abis    = abi_seg.split('.').map(parse_abi_tag).collect::<Result<Vec<_>, _>>()?;
    let plats   = plat_seg.split('.').map(parse_platform_tag).collect::<Result<Vec<_>, _>>()?;

    // Cross product, with dedup via a Vec (preserving first-seen order).
    let mut tags = Vec::new();
    for py in &pythons {
        for abi in &abis {
            for plat in &plats {
                let t = Tag { python: py.clone(), abi: abi.clone(), plat: plat.clone() };
                if !tags.contains(&t) {
                    tags.push(t);
                }
            }
        }
    }

    Ok(WheelTag { tags, raw_filename: filename.to_string() })
}

fn parse_python_tag(s: &str) -> Result<PythonTag, TagParseError> {
    // cp312, cp3, cp310, py3, py37, py2, pp310, jy27, etc.
    if let Some(rest) = s.strip_prefix("cp") {
        return parse_version_digits(rest)
            .map(|(maj, min)| PythonTag::CPython(maj, min.unwrap_or(0)))
            .ok_or_else(|| TagParseError::UnknownTagShape(s.into()));
    }
    if let Some(rest) = s.strip_prefix("py") {
        return parse_version_digits(rest)
            .map(|(maj, min)| PythonTag::Py(maj, min))
            .ok_or_else(|| TagParseError::UnknownTagShape(s.into()));
    }
    // pp, jy, ip, etc — non-CPython implementations.
    Ok(PythonTag::Other(s.to_string()))
}

fn parse_abi_tag(s: &str) -> Result<AbiTag, TagParseError> {
    match s {
        "abi3" => Ok(AbiTag::Abi3),
        "none" => Ok(AbiTag::None),
        _ => {
            if let Some(rest) = s.strip_prefix("cp") {
                // cp312, cp310d (debug), cp310m (pymalloc), etc — strip trailing letters.
                let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
                if let Some((maj, min)) = parse_version_digits(&digits) {
                    return Ok(AbiTag::CPython(maj, min.unwrap_or(0)));
                }
            }
            Ok(AbiTag::Other(s.to_string()))
        }
    }
}

fn parse_platform_tag(s: &str) -> Result<PlatformTag, TagParseError> {
    if s == "any" {
        return Ok(PlatformTag::Any);
    }
    // manylinux aliases first.
    if let Some(arch) = s.strip_prefix("manylinux1_") {
        return Ok(PlatformTag::ManyLinux { major: 2, minor: 5, arch: parse_linux_arch(arch)? });
    }
    if let Some(arch) = s.strip_prefix("manylinux2010_") {
        return Ok(PlatformTag::ManyLinux { major: 2, minor: 12, arch: parse_linux_arch(arch)? });
    }
    if let Some(arch) = s.strip_prefix("manylinux2014_") {
        return Ok(PlatformTag::ManyLinux { major: 2, minor: 17, arch: parse_linux_arch(arch)? });
    }
    if let Some(rest) = s.strip_prefix("manylinux_") {
        let (maj, min, arch) = split_versioned_linux(rest)
            .ok_or_else(|| TagParseError::UnknownTagShape(s.into()))?;
        return Ok(PlatformTag::ManyLinux { major: maj, minor: min, arch });
    }
    if let Some(rest) = s.strip_prefix("musllinux_") {
        let (maj, min, arch) = split_versioned_linux(rest)
            .ok_or_else(|| TagParseError::UnknownTagShape(s.into()))?;
        return Ok(PlatformTag::MuslLinux { major: maj, minor: min, arch });
    }
    if let Some(rest) = s.strip_prefix("macosx_") {
        let (maj, min, arch) = split_versioned_mac(rest)
            .ok_or_else(|| TagParseError::UnknownTagShape(s.into()))?;
        return Ok(PlatformTag::MacOs { major: maj, minor: min, arch });
    }
    Ok(PlatformTag::Other(s.to_string()))
}

fn parse_linux_arch(s: &str) -> Result<LinuxArch, TagParseError> {
    match s {
        "x86_64" => Ok(LinuxArch::X86_64),
        "aarch64" => Ok(LinuxArch::Aarch64),
        _ => Err(TagParseError::UnknownTagShape(s.into())),
    }
}

/// Parse `<major>_<minor>_<arch>` for manylinux/musllinux.
fn split_versioned_linux(s: &str) -> Option<(u32, u32, LinuxArch)> {
    // s = "2_17_x86_64" or "1_2_aarch64"
    let mut parts = s.splitn(3, '_');
    let major: u32 = parts.next()?.parse().ok()?;
    let minor: u32 = parts.next()?.parse().ok()?;
    let arch_str = parts.next()?;
    let arch = match arch_str {
        "x86_64" => LinuxArch::X86_64,
        "aarch64" => LinuxArch::Aarch64,
        _ => return None,
    };
    Some((major, minor, arch))
}

/// Parse `<major>_<minor>_<arch>` for macosx.
fn split_versioned_mac(s: &str) -> Option<(u32, u32, MacArch)> {
    let mut parts = s.splitn(3, '_');
    let major: u32 = parts.next()?.parse().ok()?;
    let minor: u32 = parts.next()?.parse().ok()?;
    let arch_str = parts.next()?;
    let arch = match arch_str {
        "x86_64" => MacArch::X86_64,
        "arm64" => MacArch::Arm64,
        "universal2" => MacArch::Universal2,
        _ => return None,
    };
    Some((major, minor, arch))
}

/// Parse the digit string after `cp` or `py`. Returns (major, optional minor).
/// `"3"` → (3, None); `"312"` → (3, Some(12)); `"37"` → (3, Some(7)).
fn parse_version_digits(s: &str) -> Option<(u8, Option<u8>)> {
    if s.is_empty() || !s.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    // First digit is major. If only 1 digit, minor is None.
    let major: u8 = s[..1].parse().ok()?;
    if s.len() == 1 {
        return Some((major, None));
    }
    let minor: u8 = s[1..].parse().ok()?;
    Some((major, Some(minor)))
}
```

- [ ] **Step 4: Run — all tests pass**

Run: `cargo test --lib wheel::tag`
Expected: all 11 new tests PASS.

- [ ] **Step 5: Commit**

```bash
git add src/wheel/tag.rs
git commit -m "feat(s2): parse_filename with PEP 600/656 alias canonicalization

Parses PEP 427 wheel filenames, expanding compressed tag sets and
canonicalizing manylinux1/2010/2014 aliases at parse time. Unsupported
tags (PyPy, Jython, novel platforms) are kept as Other(String) so the
parser never errors on real-world wheels."
```

---

## Phase 4 — Compatible-tag construction

### Task 9: `CompatibleTags` storage + `rank_of`

**Files:**
- Create: `src/wheel/compat.rs`
- Modify: `src/wheel/mod.rs` (export new types)

The `CompatibleTags` struct holds an ordered `Vec<Tag>` and a `HashMap<Tag, usize>` for O(1) `rank_of`. Build helper kept private to the module for now; public `build_compatible_tags` lands in Task 13.

- [ ] **Step 1: Write the failing test**

Create `src/wheel/compat.rs`:

```rust
//! Compatible-tag list construction and ranking.

use super::tag::{Tag, PythonTag, AbiTag, PlatformTag, LinuxArch};
use std::collections::HashMap;

pub struct CompatibleTags {
    ordered: Vec<Tag>,
    by_tag: HashMap<Tag, usize>,
}

impl CompatibleTags {
    pub(crate) fn from_ordered(ordered: Vec<Tag>) -> Self {
        let by_tag = ordered.iter().enumerate().map(|(i, t)| (t.clone(), i)).collect();
        Self { ordered, by_tag }
    }

    pub fn rank_of(&self, tag: &Tag) -> Option<usize> {
        self.by_tag.get(tag).copied()
    }

    pub fn ordered(&self) -> &[Tag] {
        &self.ordered
    }

    pub fn len(&self) -> usize {
        self.ordered.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rank_of_returns_index() {
        let tag_a = Tag {
            python: PythonTag::CPython(3, 12),
            abi:    AbiTag::CPython(3, 12),
            plat:   PlatformTag::Any,
        };
        let tag_b = Tag {
            python: PythonTag::Py(3, None),
            abi:    AbiTag::None,
            plat:   PlatformTag::Any,
        };
        let compat = CompatibleTags::from_ordered(vec![tag_a.clone(), tag_b.clone()]);
        assert_eq!(compat.rank_of(&tag_a), Some(0));
        assert_eq!(compat.rank_of(&tag_b), Some(1));

        let unknown = Tag {
            python: PythonTag::CPython(3, 10),
            abi:    AbiTag::None,
            plat:   PlatformTag::ManyLinux { major: 2, minor: 17, arch: LinuxArch::X86_64 },
        };
        assert_eq!(compat.rank_of(&unknown), None);
    }
}
```

Update `src/wheel/mod.rs`:

```rust
pub mod tag;
pub mod compat;

pub use tag::{
    AbiTag, LinuxArch, MacArch, PlatformTag, PythonTag, Tag, TagParseError, WheelTag,
};
pub use compat::CompatibleTags;
```

- [ ] **Step 2: Run — passes**

Run: `cargo test --lib wheel::compat`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add src/wheel/compat.rs src/wheel/mod.rs
git commit -m "feat(s2): CompatibleTags storage + rank_of

Newtype around Vec<Tag> with a parallel HashMap<Tag, usize> for O(1)
rank lookup. Builder functions land in subsequent tasks."
```

---

### Task 10: `build_python_axis`

**Files:**
- Modify: `src/wheel/compat.rs`

Build the python-axis tag pairs for a target `PythonVersion`. The result is a `Vec<(PythonTag, AbiTag)>` — platform component added later in Task 13.

- [ ] **Step 1: Write the failing test**

In `src/wheel/compat.rs::tests`:

```rust
#[test]
fn python_axis_for_3_12() {
    use crate::config::PythonVersion;
    let axis = build_python_axis(PythonVersion(3, 12));

    // The first three are the current-interpreter triple.
    assert_eq!(axis[0], (PythonTag::CPython(3, 12), AbiTag::CPython(3, 12)));
    assert_eq!(axis[1], (PythonTag::CPython(3, 12), AbiTag::Abi3));
    assert_eq!(axis[2], (PythonTag::CPython(3, 12), AbiTag::None));

    // Older cp_abi3 entries: cp311-abi3 ... cp32-abi3 (10 entries).
    assert_eq!(axis[3], (PythonTag::CPython(3, 11), AbiTag::Abi3));
    assert_eq!(axis[12], (PythonTag::CPython(3, 2), AbiTag::Abi3));

    // Then py3<minor>-none walking down to py30-none.
    assert_eq!(axis[13], (PythonTag::Py(3, Some(12)), AbiTag::None));
    assert_eq!(axis[25], (PythonTag::Py(3, Some(0)), AbiTag::None));

    // Then py3-none.
    assert_eq!(axis[26], (PythonTag::Py(3, None), AbiTag::None));

    // Total length: 3 (current) + 10 (older abi3) + 13 (py3X) + 1 (py3) = 27.
    assert_eq!(axis.len(), 27);
}

#[test]
fn python_axis_for_3_8() {
    use crate::config::PythonVersion;
    let axis = build_python_axis(PythonVersion(3, 8));
    // 3 (current) + 6 (older abi3: cp37 down to cp32) + 9 (py38 down to py30) + 1 (py3) = 19
    assert_eq!(axis.len(), 19);
    assert_eq!(axis[0], (PythonTag::CPython(3, 8), AbiTag::CPython(3, 8)));
}
```

- [ ] **Step 2: Run — verify fails**

Run: `cargo test --lib wheel::compat::tests::python_axis_for_3_12`
Expected: FAIL — `build_python_axis` doesn't exist.

- [ ] **Step 3: Implement**

Add to `src/wheel/compat.rs`:

```rust
use crate::config::PythonVersion;

pub(crate) fn build_python_axis(py: PythonVersion) -> Vec<(PythonTag, AbiTag)> {
    let major = py.0;
    let minor = py.1;
    let mut out = Vec::new();

    // 1-3: Current interpreter, full / stable-ABI / no-ABI.
    out.push((PythonTag::CPython(major, minor), AbiTag::CPython(major, minor)));
    out.push((PythonTag::CPython(major, minor), AbiTag::Abi3));
    out.push((PythonTag::CPython(major, minor), AbiTag::None));

    // 4: Older cp3X-abi3, walking back from (minor-1) down to 2.
    //    PEP 384 abi3 introduced in 3.2.
    if minor >= 3 {
        for older in (2..minor).rev() {
            out.push((PythonTag::CPython(major, older), AbiTag::Abi3));
        }
    }

    // 5: py3X-none from current minor down to 0.
    for older in (0..=minor).rev() {
        out.push((PythonTag::Py(major, Some(older)), AbiTag::None));
    }

    // 6: py3-none (catch-all).
    out.push((PythonTag::Py(major, None), AbiTag::None));

    out
}
```

- [ ] **Step 4: Run — passes**

Run: `cargo test --lib wheel::compat::tests::python_axis_for_3_12 wheel::compat::tests::python_axis_for_3_8`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/wheel/compat.rs
git commit -m "feat(s2): build_python_axis enumerates compatible (py, abi) pairs

Walks PEP 425's python axis for a given PythonVersion: current
interpreter (full/abi3/none), older cp3X-abi3 entries down to 3.2,
then py3X-none from current down to 3.0, then py3-none."
```

---

### Task 11: Platform-axis builders

**Files:**
- Modify: `src/wheel/compat.rs`

Three platform builders: Linux gnu (manylinux), Linux musl (musllinux), macOS. Each takes a baseline + arch and emits the platform-axis list (including `Any` as the last entry — done at combine time, not here).

- [ ] **Step 1: Write the failing tests**

In `src/wheel/compat.rs::tests`:

```rust
#[test]
fn manylinux_axis_2_28_x86_64() {
    let axis = build_manylinux_axis((2, 28), LinuxArch::X86_64);
    // 2_28 down to 2_5 = 24 entries.
    assert_eq!(axis.len(), 24);
    assert_eq!(axis[0], PlatformTag::ManyLinux { major: 2, minor: 28, arch: LinuxArch::X86_64 });
    assert_eq!(axis[23], PlatformTag::ManyLinux { major: 2, minor: 5, arch: LinuxArch::X86_64 });
}

#[test]
fn musllinux_axis_1_2_aarch64() {
    let axis = build_musllinux_axis((1, 2), LinuxArch::Aarch64);
    // 1_2 down to 1_0 = 3 entries.
    assert_eq!(axis.len(), 3);
    assert_eq!(axis[0], PlatformTag::MuslLinux { major: 1, minor: 2, arch: LinuxArch::Aarch64 });
    assert_eq!(axis[2], PlatformTag::MuslLinux { major: 1, minor: 0, arch: LinuxArch::Aarch64 });
}

#[test]
fn macos_axis_11_0_arm64() {
    let axis = build_macos_axis((11, 0), MacArch::Arm64);
    // First entry: 11_0 arm64.
    assert_eq!(axis[0], PlatformTag::MacOs { major: 11, minor: 0, arch: MacArch::Arm64 });
    // Second entry: 11_0 universal2.
    assert_eq!(axis[1], PlatformTag::MacOs { major: 11, minor: 0, arch: MacArch::Universal2 });
    // Then 10.16 arm64, 10.16 universal2, 10.15 arm64, 10.15 universal2, ...
    assert_eq!(axis[2], PlatformTag::MacOs { major: 10, minor: 16, arch: MacArch::Arm64 });
    // 10.16 down to 10.4 = 13 minors, each with 2 arches = 26 entries.
    // Plus the macos_min entry = 28 total.
    assert_eq!(axis.len(), 28);
    // No entries above 11_0.
    assert!(!axis.iter().any(|t| matches!(t,
        PlatformTag::MacOs { major: m, .. } if *m > 11)));
}

#[test]
fn macos_axis_14_0_x86_64() {
    let axis = build_macos_axis((14, 0), MacArch::X86_64);
    // 14_0, 14_0_universal2, 13_0, 13_0_universal2, 12_0, 12_0_universal2,
    // 11_0, 11_0_universal2, then 10.16 down to 10.4 (× 2 arches).
    // = 4 × 2 + 13 × 2 = 34.
    assert_eq!(axis.len(), 34);
    assert_eq!(axis[0], PlatformTag::MacOs { major: 14, minor: 0, arch: MacArch::X86_64 });
    assert_eq!(axis[2], PlatformTag::MacOs { major: 13, minor: 0, arch: MacArch::X86_64 });
}

#[test]
fn macos_axis_10_15_arm64_only_10x_range() {
    let axis = build_macos_axis((10, 15), MacArch::Arm64);
    // 10.15 down to 10.4 = 12 minors × 2 arches = 24.
    assert_eq!(axis.len(), 24);
    assert!(axis.iter().all(|t| matches!(t,
        PlatformTag::MacOs { major: 10, .. })));
}
```

- [ ] **Step 2: Run — verify all fail**

Run: `cargo test --lib wheel::compat::tests::manylinux_axis wheel::compat::tests::musllinux_axis wheel::compat::tests::macos_axis`
Expected: all FAIL.

- [ ] **Step 3: Implement**

Add to `src/wheel/compat.rs`:

```rust
use super::tag::MacArch;

const MANYLINUX_FLOOR_MINOR: u32 = 5;
const MUSLLINUX_FLOOR_MINOR: u32 = 0;
const MACOS_10_MIN_MINOR: u32 = 4;
const MACOS_10_MAX_MINOR: u32 = 16;

pub(crate) fn build_manylinux_axis(baseline: (u32, u32), arch: LinuxArch) -> Vec<PlatformTag> {
    let (major, max_minor) = baseline;
    let mut out = Vec::new();
    // Only major == 2 is in scope (manylinux uses glibc major 2.X exclusively).
    assert!(major == 2, "manylinux baseline must have major == 2");
    for n in (MANYLINUX_FLOOR_MINOR..=max_minor).rev() {
        out.push(PlatformTag::ManyLinux { major, minor: n, arch: arch.clone() });
    }
    out
}

pub(crate) fn build_musllinux_axis(baseline: (u32, u32), arch: LinuxArch) -> Vec<PlatformTag> {
    let (major, max_minor) = baseline;
    let mut out = Vec::new();
    for n in (MUSLLINUX_FLOOR_MINOR..=max_minor).rev() {
        out.push(PlatformTag::MuslLinux { major, minor: n, arch: arch.clone() });
    }
    out
}

pub(crate) fn build_macos_axis(macos_min: (u32, u32), primary: MacArch) -> Vec<PlatformTag> {
    let (min_major, min_minor) = macos_min;
    let mut out = Vec::new();

    if min_major >= 11 {
        // First entry: the exact macos_min target (most-preferred).
        out.push(PlatformTag::MacOs { major: min_major, minor: min_minor, arch: primary.clone() });
        out.push(PlatformTag::MacOs { major: min_major, minor: min_minor, arch: MacArch::Universal2 });
        // Then majors below min_major down to 11 (with minor=0).
        for m in (11..min_major).rev() {
            out.push(PlatformTag::MacOs { major: m, minor: 0, arch: primary.clone() });
            out.push(PlatformTag::MacOs { major: m, minor: 0, arch: MacArch::Universal2 });
        }
        // 10.x range: from 10.16 down to 10.4.
        for n in (MACOS_10_MIN_MINOR..=MACOS_10_MAX_MINOR).rev() {
            out.push(PlatformTag::MacOs { major: 10, minor: n, arch: primary.clone() });
            out.push(PlatformTag::MacOs { major: 10, minor: n, arch: MacArch::Universal2 });
        }
    } else {
        // min_major == 10: only the 10.x range from min_minor down to 4.
        assert!(min_major == 10, "macos_min major must be >= 10");
        for n in (MACOS_10_MIN_MINOR..=min_minor).rev() {
            out.push(PlatformTag::MacOs { major: 10, minor: n, arch: primary.clone() });
            out.push(PlatformTag::MacOs { major: 10, minor: n, arch: MacArch::Universal2 });
        }
    }
    out
}
```

- [ ] **Step 4: Run — passes**

Run: `cargo test --lib wheel::compat`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/wheel/compat.rs
git commit -m "feat(s2): platform-axis builders for manylinux/musllinux/macOS

build_manylinux_axis walks 2_<baseline> down to 2_5.
build_musllinux_axis walks 1_<baseline> down to 1_0.
build_macos_axis walks deployment target down through 10.4 (with
universal2 variants at each step); the macos_min semantics match
pip's mac_platforms(version=...) behavior — we accept wheels with
deployment target <= macos_min, not >="
```

---

### Task 12: `build_compatible_tags` — combine axes

**Files:**
- Modify: `src/wheel/compat.rs`

Combines the python axis with the platform axis (python outer, platform inner) and appends `PlatformTag::Any` so pure-python wheels match as a fallback. Returns a `CompatibleTags`. Resolves the target triple to dispatch among the three platform builders, applying the `MacArch::X86_64` vs `Arm64` primary correctly.

- [ ] **Step 1: Write the failing test**

In `src/wheel/compat.rs::tests`:

```rust
#[test]
fn build_compatible_tags_linux_gnu_3_12() {
    use crate::config::{Platform, PythonVersion};
    let platform = Platform {
        target: "x86_64-unknown-linux-gnu".into(),
        manylinux: Some("2_28".into()),
        musllinux: None,
        macos_min: None,
    };
    let compat = build_compatible_tags(&platform, PythonVersion(3, 12));

    // The most-preferred entry: cp312-cp312-manylinux_2_28_x86_64.
    assert_eq!(compat.ordered()[0], Tag {
        python: PythonTag::CPython(3, 12),
        abi:    AbiTag::CPython(3, 12),
        plat:   PlatformTag::ManyLinux { major: 2, minor: 28, arch: LinuxArch::X86_64 },
    });
    // Final entry is always py3-none-any.
    assert_eq!(compat.ordered().last(), Some(&Tag {
        python: PythonTag::Py(3, None),
        abi:    AbiTag::None,
        plat:   PlatformTag::Any,
    }));

    // A pure-py3 wheel must rank somewhere; py3-none-manylinux_2_28 should beat py3-none-any.
    let py3_manylinux = Tag {
        python: PythonTag::Py(3, None),
        abi:    AbiTag::None,
        plat:   PlatformTag::ManyLinux { major: 2, minor: 28, arch: LinuxArch::X86_64 },
    };
    let py3_any = Tag {
        python: PythonTag::Py(3, None),
        abi:    AbiTag::None,
        plat:   PlatformTag::Any,
    };
    assert!(compat.rank_of(&py3_manylinux).unwrap() < compat.rank_of(&py3_any).unwrap());
}

#[test]
fn build_compatible_tags_macos_arm64_3_12() {
    use crate::config::{Platform, PythonVersion};
    let platform = Platform {
        target: "aarch64-apple-darwin".into(),
        manylinux: None,
        musllinux: None,
        macos_min: Some("11.0".into()),
    };
    let compat = build_compatible_tags(&platform, PythonVersion(3, 12));

    // Rejects wheels for macOS 12+ (above deployment target).
    let mac_12 = Tag {
        python: PythonTag::CPython(3, 12),
        abi:    AbiTag::CPython(3, 12),
        plat:   PlatformTag::MacOs { major: 12, minor: 0, arch: MacArch::Arm64 },
    };
    assert_eq!(compat.rank_of(&mac_12), None);

    // Accepts deployment target == macos_min.
    let mac_11 = Tag {
        python: PythonTag::CPython(3, 12),
        abi:    AbiTag::CPython(3, 12),
        plat:   PlatformTag::MacOs { major: 11, minor: 0, arch: MacArch::Arm64 },
    };
    assert!(compat.rank_of(&mac_11).is_some());

    // arm64 platform does NOT accept x86_64-tagged wheels.
    let mac_11_x86 = Tag {
        python: PythonTag::CPython(3, 12),
        abi:    AbiTag::CPython(3, 12),
        plat:   PlatformTag::MacOs { major: 11, minor: 0, arch: MacArch::X86_64 },
    };
    assert_eq!(compat.rank_of(&mac_11_x86), None);

    // universal2 IS accepted on arm64 platforms.
    let mac_11_universal = Tag {
        python: PythonTag::CPython(3, 12),
        abi:    AbiTag::CPython(3, 12),
        plat:   PlatformTag::MacOs { major: 11, minor: 0, arch: MacArch::Universal2 },
    };
    assert!(compat.rank_of(&mac_11_universal).is_some());
}

#[test]
fn build_compatible_tags_linux_musl_3_11() {
    use crate::config::{Platform, PythonVersion};
    let platform = Platform {
        target: "x86_64-unknown-linux-musl".into(),
        manylinux: None,
        musllinux: Some("1_2".into()),
        macos_min: None,
    };
    let compat = build_compatible_tags(&platform, PythonVersion(3, 11));

    // musllinux baseline 1.2 should accept 1.0, 1.1, 1.2 wheels but not 1.3+.
    let m_12 = Tag {
        python: PythonTag::CPython(3, 11),
        abi:    AbiTag::CPython(3, 11),
        plat:   PlatformTag::MuslLinux { major: 1, minor: 2, arch: LinuxArch::X86_64 },
    };
    let m_11 = Tag {
        python: PythonTag::CPython(3, 11),
        abi:    AbiTag::CPython(3, 11),
        plat:   PlatformTag::MuslLinux { major: 1, minor: 1, arch: LinuxArch::X86_64 },
    };
    let m_13 = Tag {
        python: PythonTag::CPython(3, 11),
        abi:    AbiTag::CPython(3, 11),
        plat:   PlatformTag::MuslLinux { major: 1, minor: 3, arch: LinuxArch::X86_64 },
    };
    assert!(compat.rank_of(&m_12).unwrap() < compat.rank_of(&m_11).unwrap());
    assert_eq!(compat.rank_of(&m_13), None);
}
```

- [ ] **Step 2: Run — verify all fail**

Run: `cargo test --lib wheel::compat::tests::build_compatible_tags`
Expected: all FAIL.

- [ ] **Step 3: Implement**

Add to `src/wheel/compat.rs`:

```rust
pub fn build_compatible_tags(platform: &crate::config::Platform, py: PythonVersion) -> CompatibleTags {
    let python_axis = build_python_axis(py);
    let mut platform_axis = build_target_platform_axis(platform);
    // Append Any at the end so it loses to every specific platform tag but still matches.
    platform_axis.push(PlatformTag::Any);

    // Cross product: python outer, platform inner (spec §6).
    let mut ordered = Vec::with_capacity(python_axis.len() * platform_axis.len());
    for (py_tag, abi_tag) in &python_axis {
        for plat_tag in &platform_axis {
            ordered.push(Tag {
                python: py_tag.clone(),
                abi:    abi_tag.clone(),
                plat:   plat_tag.clone(),
            });
        }
    }
    CompatibleTags::from_ordered(ordered)
}

fn build_target_platform_axis(platform: &crate::config::Platform) -> Vec<PlatformTag> {
    let target = platform.target.as_str();
    let arch = if target.starts_with("x86_64") {
        LinuxArch::X86_64
    } else if target.starts_with("aarch64") {
        LinuxArch::Aarch64
    } else {
        unreachable!("Config::validate must have rejected this triple")
    };

    if target.ends_with("linux-gnu") {
        let baseline = platform.manylinux_baseline()
            .expect("validated linux-gnu platform has manylinux baseline");
        build_manylinux_axis(baseline, arch)
    } else if target.ends_with("linux-musl") {
        let baseline = platform.musllinux_baseline()
            .expect("validated linux-musl platform has musllinux baseline");
        build_musllinux_axis(baseline, arch)
    } else if target.ends_with("apple-darwin") {
        let macos_min = platform.macos_min_parsed()
            .expect("validated macOS platform has macos_min");
        let primary = if target.starts_with("x86_64") {
            MacArch::X86_64
        } else {
            MacArch::Arm64
        };
        build_macos_axis(macos_min, primary)
    } else {
        unreachable!("Config::validate must have rejected this triple")
    }
}
```

- [ ] **Step 4: Run — passes**

Run: `cargo test --lib wheel::compat`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/wheel/compat.rs
git commit -m "feat(s2): build_compatible_tags combines python + platform axes

Cross-product with python axis as outer loop, platform axis as inner.
Appends PlatformTag::Any so pure-python wheels match as final fallback.
Target-triple dispatch picks the right platform-axis builder."
```

---

### Task 13: Snapshot tests for the 5×3 matrix

**Files:**
- Modify: `src/wheel/compat.rs::tests`
- Add: `Cargo.toml` `insta` dev-dependency if not already present

Snapshot the full ordered `CompatibleTags` for the five-platform × three-python matrix. Catches reordering or off-by-one bugs that unit tests miss.

- [ ] **Step 1: Verify `insta` is already a dev-dep**

Run: `grep -A 5 "\[dev-dependencies\]" Cargo.toml | grep insta`
Expected: a line like `insta = "1.x"`. If not, add it: `cargo add --dev insta`.

- [ ] **Step 2: Write the failing snapshot test**

In `src/wheel/compat.rs::tests`:

```rust
fn format_compat_for_snapshot(compat: &CompatibleTags) -> String {
    compat.ordered().iter()
        .enumerate()
        .map(|(i, t)| format!("{:3}: {}", i, render_tag(t)))
        .collect::<Vec<_>>()
        .join("\n")
}

fn render_tag(t: &Tag) -> String {
    format!("{}-{}-{}", render_python(&t.python), render_abi(&t.abi), render_platform(&t.plat))
}

fn render_python(p: &PythonTag) -> String {
    match p {
        PythonTag::CPython(maj, min) => format!("cp{}{}", maj, min),
        PythonTag::Py(maj, Some(min)) => format!("py{}{}", maj, min),
        PythonTag::Py(maj, None) => format!("py{}", maj),
        PythonTag::Other(s) => s.clone(),
    }
}

fn render_abi(a: &AbiTag) -> String {
    match a {
        AbiTag::CPython(maj, min) => format!("cp{}{}", maj, min),
        AbiTag::Abi3 => "abi3".into(),
        AbiTag::None => "none".into(),
        AbiTag::Other(s) => s.clone(),
    }
}

fn render_platform(p: &PlatformTag) -> String {
    use super::tag::{LinuxArch, MacArch};
    let arch_l = |a: &LinuxArch| match a { LinuxArch::X86_64 => "x86_64", LinuxArch::Aarch64 => "aarch64" };
    let arch_m = |a: &MacArch| match a {
        MacArch::X86_64 => "x86_64", MacArch::Arm64 => "arm64", MacArch::Universal2 => "universal2",
    };
    match p {
        PlatformTag::Any => "any".into(),
        PlatformTag::ManyLinux { major, minor, arch } =>
            format!("manylinux_{}_{}_{}", major, minor, arch_l(arch)),
        PlatformTag::MuslLinux { major, minor, arch } =>
            format!("musllinux_{}_{}_{}", major, minor, arch_l(arch)),
        PlatformTag::MacOs { major, minor, arch } =>
            format!("macosx_{}_{}_{}", major, minor, arch_m(arch)),
        PlatformTag::Other(s) => s.clone(),
    }
}

fn platform(target: &str, mnl: Option<&str>, msl: Option<&str>, mac: Option<&str>) -> crate::config::Platform {
    crate::config::Platform {
        target: target.into(),
        manylinux: mnl.map(String::from),
        musllinux: msl.map(String::from),
        macos_min: mac.map(String::from),
    }
}

#[test]
fn compat_snapshot_linux_x86_64_gnu_3_12() {
    use crate::config::PythonVersion;
    let p = platform("x86_64-unknown-linux-gnu", Some("2_28"), None, None);
    let c = build_compatible_tags(&p, PythonVersion(3, 12));
    insta::assert_snapshot!(format_compat_for_snapshot(&c));
}

#[test]
fn compat_snapshot_linux_aarch64_gnu_3_11() {
    use crate::config::PythonVersion;
    let p = platform("aarch64-unknown-linux-gnu", Some("2_17"), None, None);
    let c = build_compatible_tags(&p, PythonVersion(3, 11));
    insta::assert_snapshot!(format_compat_for_snapshot(&c));
}

#[test]
fn compat_snapshot_linux_x86_64_musl_3_12() {
    use crate::config::PythonVersion;
    let p = platform("x86_64-unknown-linux-musl", None, Some("1_2"), None);
    let c = build_compatible_tags(&p, PythonVersion(3, 12));
    insta::assert_snapshot!(format_compat_for_snapshot(&c));
}

#[test]
fn compat_snapshot_macos_x86_64_3_12() {
    use crate::config::PythonVersion;
    let p = platform("x86_64-apple-darwin", None, None, Some("11.0"));
    let c = build_compatible_tags(&p, PythonVersion(3, 12));
    insta::assert_snapshot!(format_compat_for_snapshot(&c));
}

#[test]
fn compat_snapshot_macos_arm64_3_12() {
    use crate::config::PythonVersion;
    let p = platform("aarch64-apple-darwin", None, None, Some("11.0"));
    let c = build_compatible_tags(&p, PythonVersion(3, 12));
    insta::assert_snapshot!(format_compat_for_snapshot(&c));
}
```

- [ ] **Step 3: Run — first run records snapshots**

Run: `cargo test --lib wheel::compat::tests::compat_snapshot`
Expected: tests fail with "snapshot file missing"; insta writes `.snap.new` files. Inspect each `.snap.new` to verify ordering matches the algorithm (sample-check a few entries).

- [ ] **Step 4: Accept the snapshots**

Run: `INSTA_UPDATE=always cargo test --lib wheel::compat::tests::compat_snapshot` (or run `cargo insta accept` if `cargo-insta` is installed).
Expected: snapshots accepted; `.snap` files committed.

- [ ] **Step 5: Re-run — confirm stable**

Run: `cargo test --lib wheel::compat::tests::compat_snapshot`
Expected: PASS (no diff).

- [ ] **Step 6: Commit**

```bash
git add src/wheel/compat.rs src/wheel/snapshots/
git commit -m "test(s2): snapshot compatible-tag lists for 5x3 matrix

Five platforms (linux-x86_64-gnu, linux-aarch64-gnu, linux-x86_64-musl,
macos-x86_64, macos-arm64) at python 3.11 / 3.12 each are pinned via
insta snapshots so reordering bugs become visible diffs in review."
```

---

## Phase 5 — Selector

### Task 14: `pick_wheel` selector

**Files:**
- Create: `src/wheel/select.rs`
- Modify: `src/wheel/mod.rs` (export `PickResult`, `pick_wheel`)

- [ ] **Step 1: Write the failing test**

Create `src/wheel/select.rs`:

```rust
//! Wheel selection by min-rank lookup against a CompatibleTags list.

use crate::lock::types::Wheel;
use super::tag::{parse_filename, Tag};
use super::compat::CompatibleTags;

#[derive(Debug, Clone)]
pub enum PickResult<'a> {
    Picked { wheel: &'a Wheel, matched_tag: Tag, rank: usize },
    NoWheel,
}

pub fn pick_wheel<'a>(wheels: &'a [Wheel], compat: &CompatibleTags) -> PickResult<'a> {
    // Sort by filename for stable tie-break — removes dependence on uv.lock emission order.
    let mut sorted: Vec<&Wheel> = wheels.iter().collect();
    sorted.sort_by(|a, b| a.filename.cmp(&b.filename));

    let mut best: Option<(usize, Tag, &Wheel)> = None;

    for wheel in sorted {
        let parsed = match parse_filename(&wheel.filename) {
            Ok(t) => t,
            Err(_) => continue,
        };
        for tag in &parsed.tags {
            if let Some(rank) = compat.rank_of(tag) {
                let improves = best.as_ref().map_or(true, |(r, _, _)| rank < *r);
                if improves {
                    best = Some((rank, tag.clone(), wheel));
                }
                #[cfg(debug_assertions)]
                {
                    // Sanity-check: equal-rank tie across different wheels should never happen
                    // in practice (a wheel can't list the same Tag twice, and uv.lock shouldn't
                    // emit two wheels with identical tag triples).
                    if let Some((r, _, w_prev)) = best.as_ref() {
                        if *r == rank && !std::ptr::eq(*w_prev, wheel) {
                            debug_assert_ne!(w_prev.filename, wheel.filename,
                                "two wheels share best rank {} — uv.lock duplicates?", rank);
                        }
                    }
                }
            }
        }
    }

    match best {
        Some((rank, matched_tag, wheel)) => PickResult::Picked { wheel, matched_tag, rank },
        None => PickResult::NoWheel,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Platform, PythonVersion};
    use crate::wheel::compat::build_compatible_tags;
    use url::Url;

    fn make_wheel(filename: &str) -> Wheel {
        Wheel {
            url: Url::parse(&format!("https://example.com/{filename}")).unwrap(),
            hash: "sha256:abc".into(),
            size: None,
            filename: filename.into(),
        }
    }

    fn linux_gnu_x86() -> Platform {
        Platform {
            target: "x86_64-unknown-linux-gnu".into(),
            manylinux: Some("2_28".into()),
            musllinux: None,
            macos_min: None,
        }
    }

    #[test]
    fn picks_most_specific_wheel() {
        let wheels = vec![
            make_wheel("foo-1.0-py3-none-any.whl"),
            make_wheel("foo-1.0-cp312-abi3-manylinux_2_17_x86_64.whl"),
            make_wheel("foo-1.0-cp312-cp312-manylinux_2_17_x86_64.whl"),
        ];
        let compat = build_compatible_tags(&linux_gnu_x86(), PythonVersion(3, 12));
        let pick = pick_wheel(&wheels, &compat);
        match pick {
            PickResult::Picked { wheel, .. } =>
                assert_eq!(wheel.filename, "foo-1.0-cp312-cp312-manylinux_2_17_x86_64.whl"),
            _ => panic!("expected Picked"),
        }
    }

    #[test]
    fn returns_no_wheel_when_nothing_matches() {
        let wheels = vec![make_wheel("foo-1.0-cp310-cp310-manylinux_2_17_x86_64.whl")];
        let compat = build_compatible_tags(&linux_gnu_x86(), PythonVersion(3, 12));
        assert!(matches!(pick_wheel(&wheels, &compat), PickResult::NoWheel));
    }

    #[test]
    fn unparseable_filenames_are_skipped() {
        let wheels = vec![
            make_wheel("garbage.whl"),
            make_wheel("foo-1.0-py3-none-any.whl"),
        ];
        let compat = build_compatible_tags(&linux_gnu_x86(), PythonVersion(3, 12));
        match pick_wheel(&wheels, &compat) {
            PickResult::Picked { wheel, .. } => assert_eq!(wheel.filename, "foo-1.0-py3-none-any.whl"),
            _ => panic!("expected Picked"),
        }
    }

    #[test]
    fn input_order_does_not_affect_result() {
        // Same wheel set, two different input orders — same wheel picked.
        let w1 = make_wheel("foo-1.0-cp312-cp312-manylinux_2_17_x86_64.whl");
        let w2 = make_wheel("foo-1.0-py3-none-any.whl");
        let compat = build_compatible_tags(&linux_gnu_x86(), PythonVersion(3, 12));
        let pick_a = pick_wheel(&[w1.clone(), w2.clone()], &compat);
        let pick_b = pick_wheel(&[w2, w1], &compat);
        match (pick_a, pick_b) {
            (PickResult::Picked { wheel: a, .. }, PickResult::Picked { wheel: b, .. }) =>
                assert_eq!(a.filename, b.filename),
            _ => panic!("expected both Picked"),
        }
    }
}
```

- [ ] **Step 2: Update `src/wheel/mod.rs`**

```rust
pub mod tag;
pub mod compat;
pub mod select;

pub use tag::{
    AbiTag, LinuxArch, MacArch, PlatformTag, PythonTag, Tag, TagParseError, WheelTag,
    parse_filename,
};
pub use compat::{CompatibleTags, build_compatible_tags};
pub use select::{PickResult, pick_wheel};
```

- [ ] **Step 3: Run — passes**

Run: `cargo test --lib wheel::select`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add src/wheel/select.rs src/wheel/mod.rs
git commit -m "feat(s2): pick_wheel selector via min-rank lookup

Sorts wheels by filename for stable tie-break, then scans every
(wheel, tag) pair, tracking the minimum rank seen. Returns
PickResult::Picked or PickResult::NoWheel. Unparseable filenames are
skipped silently; the debug-assertion fires only on suspicious
duplicate-rank cases."
```

---

## Phase 6 — CLI surface

### Task 15: `PickWheels` CLI variant

**Files:**
- Modify: `src/cli/debug/mod.rs`
- Create: `src/cli/debug/pick_wheels.rs`

Add the `DebugOp::PickWheels` variant and wire dispatch.

- [ ] **Step 1: Update `src/cli/debug/mod.rs`**

Find the `DebugOp` enum:

```rust
#[derive(Debug, clap::Subcommand)]
pub enum DebugOp {
    PrintDeps(print_deps::PrintDepsArgs),
    PickWheels(pick_wheels::PickWheelsArgs),
}
```

Then in the dispatch function (probably `pub fn run(globals: &Globals, op: &DebugOp) -> Result<(), MuntjacError>`):

```rust
pub fn run(globals: &Globals, op: &DebugOp) -> Result<(), MuntjacError> {
    match op {
        DebugOp::PrintDeps(args) => print_deps::run(globals, args),
        DebugOp::PickWheels(args) => pick_wheels::run(globals, args),
    }
}
```

Add the module declaration at the top:

```rust
pub mod print_deps;
pub mod pick_wheels;
```

- [ ] **Step 2: Create the skeleton `src/cli/debug/pick_wheels.rs`**

```rust
//! `muntjac debug pick-wheels` — print the wheel selected for each
//! (package, version, platform, python_version) cell as JSON.

use crate::cli::Globals;
use crate::error::MuntjacError;

#[derive(Debug, clap::Args)]
pub struct PickWheelsArgs {
    /// Limit to a single tree (default: all).
    #[arg(long)]
    pub tree: Option<String>,
    /// Limit to a single platform (default: all).
    #[arg(long)]
    pub platform: Option<String>,
    /// Limit to a single python version (default: all).
    #[arg(long)]
    pub python: Option<String>,
    /// Limit to a single package (default: all).
    #[arg(long)]
    pub package: Option<String>,
}

pub fn run(_globals: &Globals, _args: &PickWheelsArgs) -> Result<(), MuntjacError> {
    // Stub: actual logic lands in Task 16.
    unimplemented!("pick-wheels stub")
}
```

- [ ] **Step 3: Verify compile**

Run: `cargo build`
Expected: PASS.

- [ ] **Step 4: Smoke-test help text**

Run: `cargo run -- debug pick-wheels --help`
Expected: clap prints the subcommand help with the four `--tree/--platform/--python/--package` flags.

- [ ] **Step 5: Commit**

```bash
git add src/cli/debug/mod.rs src/cli/debug/pick_wheels.rs
git commit -m "feat(s2): CLI Debug enum gains PickWheels variant

clap subcommand wired with --tree/--platform/--python/--package
filters. Handler is a stub; logic lands in the next task."
```

---

### Task 16: `pick_wheels` handler — compose pipeline + emit JSON

**Files:**
- Modify: `src/cli/debug/pick_wheels.rs`
- Modify: `src/cli/debug/print_deps.rs` (extract a shared helper if needed)

The handler must:
1. Load `muntjac.toml` from `globals.workdir()`.
2. For each tree (filtered by `--tree`), load `uv.lock`, build the dep graph, compute the resolved view.
3. For each `(package, platform, python)` cell (filtered by `--platform`/`--python`/`--package`), call `pick_wheel` and accumulate into the output struct.
4. Emit JSON to stdout with the shape from spec §9.

- [ ] **Step 1: Write the failing integration test**

In a new file `tests/pick_wheels.rs`:

```rust
//! Integration tests for `muntjac debug pick-wheels`.

use std::path::Path;
use std::process::Command;

fn run_pick_wheels(fixture_dir: &Path, extra_args: &[&str]) -> std::process::Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_muntjac"));
    cmd.arg("-C").arg(fixture_dir).args(["debug", "pick-wheels"]).args(extra_args);
    cmd.output().expect("run muntjac")
}

#[test]
fn fixture_04_pure_python_picks_any_wheel() {
    let fixture = Path::new("tests/fixtures/wheel/04-pure-python");
    let out = run_pick_wheels(fixture, &[]);
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let json: serde_json::Value = serde_json::from_slice(&out.stdout)
        .expect("output is valid JSON");
    assert_eq!(json["schema_version"], 1);
    let selections = json["selections"].as_array().expect("selections is array");
    assert!(!selections.is_empty());
    // Every selection should be 'picked' (pure-python wheels match every cell).
    for s in selections {
        assert_eq!(s["outcome"], "picked", "expected picked for {s}");
    }
}
```

We rely on fixture 04 existing for this test; fixture creation is Task 18. To keep this task self-contained, create a *minimal* version of fixture 04 here (one package with one `py3-none-any` wheel), and Task 18 extends fixtures 02/03/05.

Create `tests/fixtures/wheel/04-pure-python/muntjac.toml`:

```toml
manifest_path   = "pyproject.toml"
third_party_dir = "."
python_versions = ["3.12"]

[platforms.linux-x86_64-gnu]
target = "x86_64-unknown-linux-gnu"
manylinux = "2_28"
```

Create `tests/fixtures/wheel/04-pure-python/pyproject.toml`:

```toml
[project]
name = "fixture-04"
version = "0.1.0"
requires-python = ">=3.12"
dependencies = ["sortedcontainers"]
```

Create `tests/fixtures/wheel/04-pure-python/uv.lock`:

```toml
version = 1
revision = 3
requires-python = ">=3.12"

[[package]]
name = "fixture-04"
version = "0.1.0"
source = { virtual = "." }
dependencies = [
    { name = "sortedcontainers" },
]

[[package]]
name = "sortedcontainers"
version = "2.4.0"
source = { registry = "https://pypi.org/simple" }
wheels = [
    { url = "https://files.pythonhosted.org/packages/.../sortedcontainers-2.4.0-py2.py3-none-any.whl", hash = "sha256:0" },
]
```

(The URL doesn't need to be real for our purposes — we don't download.)

- [ ] **Step 2: Run — verify fails**

Run: `cargo test --test pick_wheels fixture_04_pure_python_picks_any_wheel`
Expected: FAIL — handler still stubbed.

- [ ] **Step 3: Implement the handler in `src/cli/debug/pick_wheels.rs`**

```rust
use crate::cli::Globals;
use crate::config::Config;
use crate::error::MuntjacError;
use crate::lock::{Lockfile, graph::DepGraph, resolved::ResolvedView};
use crate::wheel::{build_compatible_tags, pick_wheel, PickResult, Tag, PythonTag, AbiTag, PlatformTag, LinuxArch, MacArch};
use serde::Serialize;
use std::str::FromStr;

#[derive(Debug, clap::Args)]
pub struct PickWheelsArgs {
    #[arg(long)] pub tree: Option<String>,
    #[arg(long)] pub platform: Option<String>,
    #[arg(long)] pub python: Option<String>,
    #[arg(long)] pub package: Option<String>,
}

#[derive(Serialize)]
struct Output<'a> {
    schema_version: u32,
    tree: &'a str,
    selections: Vec<Selection<'a>>,
}

#[derive(Serialize)]
struct Selection<'a> {
    package: &'a str,
    version: String,
    platform: &'a str,
    python_version: String,
    #[serde(flatten)]
    outcome: SelectionOutcome<'a>,
}

#[derive(Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
enum SelectionOutcome<'a> {
    Picked {
        wheel: WheelOutput<'a>,
        matched_tag: String,
        rank: usize,
        wheels_considered: usize,
    },
    NoWheel {
        wheels_considered: usize,
        wheels: Vec<&'a str>,
    },
}

#[derive(Serialize)]
struct WheelOutput<'a> {
    filename: &'a str,
    url: String,
    hash: &'a str,
}

pub fn run(globals: &Globals, args: &PickWheelsArgs) -> Result<(), MuntjacError> {
    let workdir = globals.workdir().map_err(MuntjacError::Io)?;
    let config_text = std::fs::read_to_string(workdir.join("muntjac.toml"))
        .map_err(MuntjacError::Io)?;
    let config = Config::from_str(&config_text).map_err(MuntjacError::Config)?;
    config.validate().map_err(MuntjacError::Config)?;

    let mut all_output = Vec::new();

    for tree in &config.trees {
        if let Some(name) = &args.tree {
            if &tree.name != name { continue; }
        }
        let lock_path = workdir.join(&tree.manifest_path).parent()
            .unwrap_or(workdir.as_path())
            .join("uv.lock");
        let lock_text = std::fs::read_to_string(&lock_path).map_err(MuntjacError::Io)?;
        let lockfile = Lockfile::from_str(&lock_text).map_err(MuntjacError::Lockfile)?;
        let graph = DepGraph::build(&lockfile).map_err(MuntjacError::Lockfile)?;
        graph.detect_cycles_or_err()?;  // existing helper that returns Err if cycles

        let mut selections = Vec::new();

        for (plat_name, plat) in &config.platforms {
            if let Some(filter) = &args.platform {
                if plat_name != filter { continue; }
            }
            for py in &tree.python_versions {
                if let Some(filter) = &args.python {
                    if &format!("{}.{}", py.0, py.1) != filter { continue; }
                }
                let resolved = ResolvedView::project(&graph, &lockfile, plat, *py,
                    &config.lockfile.include_groups);
                let compat = build_compatible_tags(plat, *py);

                for pkg in &resolved.packages {
                    if let Some(filter) = &args.package {
                        if pkg.name != *filter { continue; }
                    }
                    // Skip first-party (no wheels).
                    if pkg.wheels.is_empty() { continue; }

                    let outcome = match pick_wheel(&pkg.wheels, &compat) {
                        PickResult::Picked { wheel, matched_tag, rank } => {
                            SelectionOutcome::Picked {
                                wheel: WheelOutput {
                                    filename: &wheel.filename,
                                    url: wheel.url.to_string(),
                                    hash: &wheel.hash,
                                },
                                matched_tag: render_tag(&matched_tag),
                                rank,
                                wheels_considered: pkg.wheels.len(),
                            }
                        }
                        PickResult::NoWheel => SelectionOutcome::NoWheel {
                            wheels_considered: pkg.wheels.len(),
                            wheels: pkg.wheels.iter().map(|w| w.filename.as_str()).collect(),
                        }
                    };
                    selections.push(Selection {
                        package: &pkg.name,
                        version: pkg.version.to_string(),
                        platform: plat_name,
                        python_version: format!("{}.{}", py.0, py.1),
                        outcome,
                    });
                }
            }
        }

        // Sort selections by (package, version, platform, python_version) for determinism.
        selections.sort_by(|a, b| {
            a.package.cmp(b.package)
                .then(a.version.cmp(&b.version))
                .then(a.platform.cmp(b.platform))
                .then(a.python_version.cmp(&b.python_version))
        });

        all_output.push(Output {
            schema_version: 1,
            tree: &tree.name,
            selections,
        });
    }

    // If a single tree was requested, output that object; otherwise wrap in an array.
    let output_json = if all_output.len() == 1 {
        serde_json::to_string_pretty(&all_output[0]).map_err(MuntjacError::Json)?
    } else {
        serde_json::to_string_pretty(&all_output).map_err(MuntjacError::Json)?
    };
    println!("{}", output_json);

    Ok(())
}

fn render_tag(t: &Tag) -> String {
    let python = match &t.python {
        PythonTag::CPython(maj, min) => format!("cp{maj}{min}"),
        PythonTag::Py(maj, Some(min)) => format!("py{maj}{min}"),
        PythonTag::Py(maj, None) => format!("py{maj}"),
        PythonTag::Other(s) => s.clone(),
    };
    let abi = match &t.abi {
        AbiTag::CPython(maj, min) => format!("cp{maj}{min}"),
        AbiTag::Abi3 => "abi3".into(),
        AbiTag::None => "none".into(),
        AbiTag::Other(s) => s.clone(),
    };
    let plat = match &t.plat {
        PlatformTag::Any => "any".into(),
        PlatformTag::ManyLinux { major, minor, arch } => {
            let a = match arch { LinuxArch::X86_64 => "x86_64", LinuxArch::Aarch64 => "aarch64" };
            format!("manylinux_{major}_{minor}_{a}")
        }
        PlatformTag::MuslLinux { major, minor, arch } => {
            let a = match arch { LinuxArch::X86_64 => "x86_64", LinuxArch::Aarch64 => "aarch64" };
            format!("musllinux_{major}_{minor}_{a}")
        }
        PlatformTag::MacOs { major, minor, arch } => {
            let a = match arch {
                MacArch::X86_64 => "x86_64", MacArch::Arm64 => "arm64", MacArch::Universal2 => "universal2",
            };
            format!("macosx_{major}_{minor}_{a}")
        }
        PlatformTag::Other(s) => s.clone(),
    };
    format!("{python}-{abi}-{plat}")
}
```

Notes: `ResolvedView::project` may differ in shape from what's pseudocoded above. Check `src/lock/resolved.rs` and adapt — the key inputs/outputs you need are: per-`(plat, py)` iteration giving you a list of `(name, version, wheels)`. If the existing project signature doesn't fit, add a helper rather than rewriting `project`. The integration test in this task uses fixture 04 to confirm the end-to-end pipeline runs.

Also: `graph.detect_cycles_or_err()` may not exist as named — adapt to whatever the existing pattern is in `src/cli/debug/print_deps.rs`. Mirror that file's prelude / handler shape.

If `MuntjacError::Json` doesn't exist yet, add it: `Json(serde_json::Error)` variant with appropriate `From` derive.

- [ ] **Step 4: Run — passes**

Run: `cargo test --test pick_wheels`
Expected: PASS.

- [ ] **Step 5: Smoke-test in manual mode**

Run: `cargo run -- -C tests/fixtures/wheel/04-pure-python debug pick-wheels`
Expected: JSON output with `schema_version: 1`, `tree: "default"`, and one or more `selections` with `outcome: "picked"`.

- [ ] **Step 6: Commit**

```bash
git add src/cli/debug/pick_wheels.rs src/error.rs tests/pick_wheels.rs tests/fixtures/wheel/04-pure-python/
git commit -m "feat(s2): pick-wheels handler emits JSON for every cell

Loads config, walks each tree's resolved view per (platform,
python_version), runs pick_wheel against each package's wheels.
Output JSON shape matches spec §9: schema_version 1, sorted
selections array, outcome is picked or no_wheel.

First fixture (04-pure-python) lands here as the integration-test
smoke target."
```

---

## Phase 7 — Integration fixtures

### Task 17: Fixture 01 — numpy matrix golden

**Files:**
- Create: `tests/fixtures/wheel/01-numpy-matrix/muntjac.toml`
- Create: `tests/fixtures/wheel/01-numpy-matrix/pyproject.toml`
- Create: `tests/fixtures/wheel/01-numpy-matrix/uv.lock`
- Create: `tests/fixtures/wheel/01-numpy-matrix/expected.json`
- Modify: `tests/pick_wheels.rs` (add fixture 01 test)

This fixture is the canonical demo. Real numpy 2.1.3 wheel list (~18 wheels) across the five-platform × three-python matrix.

- [ ] **Step 1: Generate the real uv.lock**

In a scratch directory, run:

```bash
mkdir /tmp/muntjac-fixture-01 && cd /tmp/muntjac-fixture-01
cat > pyproject.toml <<EOF
[project]
name = "fixture-01"
version = "0.1.0"
requires-python = ">=3.11"
dependencies = ["numpy==2.1.3"]
EOF
uv lock
```

Copy `pyproject.toml` and `uv.lock` into `tests/fixtures/wheel/01-numpy-matrix/`.

- [ ] **Step 2: Write `muntjac.toml`**

```toml
manifest_path   = "pyproject.toml"
third_party_dir = "."
python_versions = ["3.11", "3.12", "3.13"]

[platforms.linux-x86_64-gnu]
target = "x86_64-unknown-linux-gnu"
manylinux = "2_17"

[platforms.linux-aarch64-gnu]
target = "aarch64-unknown-linux-gnu"
manylinux = "2_17"

[platforms.linux-x86_64-musl]
target = "x86_64-unknown-linux-musl"
musllinux = "1_2"

[platforms.macos-x86_64]
target = "x86_64-apple-darwin"
macos_min = "10.13"

[platforms.macos-arm64]
target = "aarch64-apple-darwin"
macos_min = "11.0"
```

- [ ] **Step 3: Add the integration test**

In `tests/pick_wheels.rs`:

```rust
#[test]
fn fixture_01_numpy_matrix_golden() {
    let fixture = Path::new("tests/fixtures/wheel/01-numpy-matrix");
    let out = run_pick_wheels(fixture, &[]);
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let actual = String::from_utf8(out.stdout).unwrap();
    let expected = std::fs::read_to_string(fixture.join("expected.json")).unwrap();
    assert_eq!(actual.trim(), expected.trim(), "golden mismatch");
}
```

- [ ] **Step 4: Generate `expected.json`**

Run: `cargo run -- -C tests/fixtures/wheel/01-numpy-matrix debug pick-wheels > tests/fixtures/wheel/01-numpy-matrix/expected.json`

Manually spot-check the output:
- For `linux-x86_64-gnu`, all three python versions: numpy should be `picked` with a `cp3X-cp3X-manylinux_2_17_x86_64.whl` wheel.
- For `linux-aarch64-gnu`, same but `aarch64` arch.
- For `linux-x86_64-musl`, depends on whether numpy 2.1.3 has musllinux wheels — if not, `outcome: "no_wheel"`.
- For both macOS platforms, expect `macosx_*_arm64` or `macosx_*_x86_64` matches.

- [ ] **Step 5: Run — passes**

Run: `cargo test --test pick_wheels fixture_01_numpy_matrix_golden`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add tests/fixtures/wheel/01-numpy-matrix/ tests/pick_wheels.rs
git commit -m "test(s2): fixture 01 — numpy 2.1.3 wheel-selection golden

Real numpy 2.1.3 uv.lock + 5-platform x 3-python matrix golden.
Covers the roadmap's headline demo for S2."
```

---

### Task 18: Fixtures 02, 03, 05

**Files:**
- Create: `tests/fixtures/wheel/02-musllinux-only/`
- Create: `tests/fixtures/wheel/03-no-wheel/`
- Create: `tests/fixtures/wheel/05-determinism/`
- Modify: `tests/pick_wheels.rs`

Fixture 04 (pure-python) was created in Task 16. Fixture 01 (numpy matrix) was Task 17. Now the remaining three.

- [ ] **Step 1: Build fixture 02 (musllinux-only)**

Create `tests/fixtures/wheel/02-musllinux-only/` with `muntjac.toml`, `pyproject.toml`, `uv.lock`, `expected.json`. Use a real package with musllinux-only wheels (e.g. an old `cryptography` or a hand-crafted minimal lockfile). Simplest path: hand-craft a minimal lockfile:

`muntjac.toml`:

```toml
manifest_path   = "pyproject.toml"
third_party_dir = "."
python_versions = ["3.12"]

[platforms.linux-x86_64-gnu]
target = "x86_64-unknown-linux-gnu"
manylinux = "2_17"

[platforms.linux-x86_64-musl]
target = "x86_64-unknown-linux-musl"
musllinux = "1_2"
```

`pyproject.toml`:

```toml
[project]
name = "fixture-02"
version = "0.1.0"
requires-python = ">=3.12"
dependencies = ["musl-only-pkg"]
```

`uv.lock`:

```toml
version = 1
revision = 3
requires-python = ">=3.12"

[[package]]
name = "fixture-02"
version = "0.1.0"
source = { virtual = "." }
dependencies = [
    { name = "musl-only-pkg" },
]

[[package]]
name = "musl-only-pkg"
version = "0.1.0"
source = { registry = "https://pypi.org/simple" }
wheels = [
    { url = "https://example.com/musl_only_pkg-0.1.0-cp312-cp312-musllinux_1_2_x86_64.whl", hash = "sha256:0" },
]
```

Hand-craft will need uv-lock validation to be lenient (it may not be — see if `Lockfile::from_str` accepts this). If uv.lock from a non-existent package fails parser checks, drop in an uv-generated lock by adding a real package with only musllinux wheels (or by modifying a real lockfile manually). The simpler workaround: depend on a known-musllinux-only package, run `uv lock`, hand-trim to just that package's wheels.

Generate `expected.json` as in Task 17. Spot-check: musl platform should pick the musllinux wheel; gnu platform should report `no_wheel`.

- [ ] **Step 2: Build fixture 03 (no-wheel)**

Same approach as 02: a package with `cp310` wheels only, target python 3.12 → all cells should be `no_wheel` with `wheels_considered > 0`.

- [ ] **Step 3: Build fixture 05 (determinism)**

This fixture *reuses* the inputs of `01-numpy-matrix` but is a separate test that runs pick-wheels twice and byte-compares.

Add to `tests/pick_wheels.rs`:

```rust
#[test]
fn fixture_05_determinism_two_runs_byte_identical() {
    let fixture = Path::new("tests/fixtures/wheel/01-numpy-matrix");
    let out1 = run_pick_wheels(fixture, &[]);
    let out2 = run_pick_wheels(fixture, &[]);
    assert_eq!(out1.stdout, out2.stdout, "two runs produced different output");
}
```

(No new fixture directory needed — the determinism check sits in `tests/pick_wheels.rs` alongside the fixture 01 golden assertion.)

- [ ] **Step 4: Add tests for fixtures 02 and 03**

In `tests/pick_wheels.rs`:

```rust
#[test]
fn fixture_02_musllinux_only() {
    let fixture = Path::new("tests/fixtures/wheel/02-musllinux-only");
    let out = run_pick_wheels(fixture, &[]);
    assert!(out.status.success());
    let actual = String::from_utf8(out.stdout).unwrap();
    let expected = std::fs::read_to_string(fixture.join("expected.json")).unwrap();
    assert_eq!(actual.trim(), expected.trim());
}

#[test]
fn fixture_03_no_wheel() {
    let fixture = Path::new("tests/fixtures/wheel/03-no-wheel");
    let out = run_pick_wheels(fixture, &[]);
    assert!(out.status.success());
    let actual = String::from_utf8(out.stdout).unwrap();
    let expected = std::fs::read_to_string(fixture.join("expected.json")).unwrap();
    assert_eq!(actual.trim(), expected.trim());
    // Sanity: every selection should be no_wheel.
    let json: serde_json::Value = serde_json::from_str(&actual).unwrap();
    for sel in json["selections"].as_array().unwrap() {
        assert_eq!(sel["outcome"], "no_wheel");
        assert!(sel["wheels_considered"].as_u64().unwrap() > 0,
            "wheels_considered should be >0 to distinguish from parse-fail");
    }
}
```

- [ ] **Step 5: Run all integration tests — pass**

Run: `cargo test --test pick_wheels`
Expected: all four tests (fixtures 01, 02, 03, 04, 05-determinism) PASS.

- [ ] **Step 6: Commit**

```bash
git add tests/fixtures/wheel/ tests/pick_wheels.rs
git commit -m "test(s2): fixtures 02 (musllinux), 03 (no-wheel), 05 (determinism)

Round out the integration coverage: musllinux-only package matches
only on musl targets; cp310-only package on cp312 target produces
no_wheel with non-empty wheels_considered; two consecutive runs on
fixture 01 are byte-identical (determinism)."
```

---

## Phase 8 — Housekeeping

### Task 19: Fixture conventions READMEs

**Files:**
- Create: `tests/fixtures/lock/README.md`
- Create: `tests/fixtures/wheel/README.md`

- [ ] **Step 1: Write `tests/fixtures/lock/README.md`**

```markdown
# Lockfile fixtures

Each directory contains:
- `muntjac.toml` — config
- `uv.lock` — **frozen artifact** (do not regenerate without also regenerating goldens)
- `pyproject.toml` (sometimes) — used to regenerate `uv.lock` if needed
- `expected-*.json` / `expected-*.txt` — golden outputs

## Regenerating a fixture

If you need to update a fixture (e.g. because the on-disk `uv.lock` needs
to reflect a newer upstream version), regenerate **both** the lockfile and the
matching golden(s) in a single commit. Reviewers should diff both. If you
regenerate only the lockfile, the test will fail with a phantom mismatch.

## Why the lockfiles are committed

`uv lock` resolves against live PyPI. If our tests called `uv lock`, they
would break whenever PyPI changes (yanked releases, new versions). Committing
the resolved lockfile freezes the test inputs.
```

- [ ] **Step 2: Write `tests/fixtures/wheel/README.md`**

```markdown
# Wheel-selection fixtures

Each directory contains:
- `muntjac.toml` — config (platforms + python versions to exercise)
- `uv.lock` — **frozen artifact**
- `pyproject.toml` — input used to regenerate `uv.lock` if needed
- `expected.json` — golden output of `muntjac debug pick-wheels`

See `tests/fixtures/lock/README.md` for the broader convention. The same
"regenerate lockfile + golden in one commit" rule applies here.

## When to update `expected.json` only

If you change the wheel-selection algorithm or output JSON shape, regenerate
all goldens (`cargo run -- -C tests/fixtures/wheel/XX debug pick-wheels >
tests/fixtures/wheel/XX/expected.json`) and commit the diffs. The PR description
should call out the behavior change.

## When to update `uv.lock` only

Don't. Always regenerate the matching `expected.json` in the same commit.
```

- [ ] **Step 3: Commit**

```bash
git add tests/fixtures/lock/README.md tests/fixtures/wheel/README.md
git commit -m "docs(s2): fixture conventions for lock/ and wheel/

Both fixture trees follow a frozen-artifacts convention. The READMEs
spell out: lockfiles are committed; regenerating them requires also
regenerating goldens; reviewers diff both.

Closes TECH_DEBT item: 'Fixture goldens are version-pinned to current PyPI'."
```

---

### Task 20: `include_groups` dedup with warning

**Files:**
- Modify: `src/config.rs::validate` (or `Config::from_raw` — whichever runs once)
- Modify: `src/config.rs::tests`

- [ ] **Step 1: Write the failing test**

In `src/config.rs::tests`:

```rust
#[test]
fn include_groups_dedups_with_warning() {
    let toml_str = r#"
manifest_path   = "../pyproject.toml"
third_party_dir = "."
python_versions = ["3.12"]

[platforms.linux-x86_64-gnu]
target = "x86_64-unknown-linux-gnu"
manylinux = "2_17"

[lockfile]
include_groups = ["test", "test", "docs"]
"#;
    let mut config = Config::from_str(toml_str).expect("parse");
    config.dedupe_include_groups();  // method we'll add
    assert_eq!(config.lockfile.include_groups, vec!["test".to_string(), "docs".to_string()]);
}
```

- [ ] **Step 2: Run — verify fails**

Run: `cargo test --lib config::tests::include_groups_dedups_with_warning`
Expected: FAIL — method doesn't exist.

- [ ] **Step 3: Implement**

In `src/config.rs`:

```rust
impl Config {
    /// Removes duplicate entries from `lockfile.include_groups`, preserving order.
    /// Emits a warning to stderr if duplicates were present.
    pub fn dedupe_include_groups(&mut self) {
        let original_len = self.lockfile.include_groups.len();
        let mut seen = std::collections::HashSet::new();
        self.lockfile.include_groups.retain(|g| seen.insert(g.clone()));
        if self.lockfile.include_groups.len() < original_len {
            eprintln!(
                "warning: muntjac.toml [lockfile] include_groups contained duplicates ({} → {}); deduplicated",
                original_len,
                self.lockfile.include_groups.len()
            );
        }
    }
}
```

Then call it from `Config::from_str` right after `from_raw`:

```rust
impl FromStr for Config {
    type Err = crate::error::ConfigError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let raw: RawConfig = toml::from_str(s).map_err(crate::error::ConfigError::Parse)?;
        let mut config = Self::from_raw(raw)?;
        config.dedupe_include_groups();
        Ok(config)
    }
}
```

- [ ] **Step 4: Run — passes**

Run: `cargo test --lib config::tests::include_groups_dedups_with_warning`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/config.rs
git commit -m "fix(s2): dedup include_groups with stderr warning

Duplicates in [lockfile] include_groups (e.g. ['test', 'test']) are
now deduplicated, preserving order. A stderr warning fires if anything
was dropped so config typos surface.

Closes TECH_DEBT item: 'include_groups duplicates are silently preserved'."
```

---

### Task 21: Documentation polish (marker_matches + extras asymmetry)

**Files:**
- Modify: `src/platform.rs` (`marker_matches` docs)
- Modify: `src/lock/graph.rs` (`reachable_with_extras` docs)

- [ ] **Step 1: Update `marker_matches` doc comment in `src/platform.rs`**

Find the `pub fn marker_matches(marker: Option<&MarkerTree>, env: &MarkerEnvironment) -> bool` and add a doc comment:

```rust
/// Evaluate a PEP 508 marker against a target environment.
///
/// This is the canonical helper for *non-edge* marker checks — e.g. checking
/// `requires-python` constraints, or evaluating markers that don't come from a
/// dependency edge. For the common case of "should we follow this dep edge?",
/// use `graph::edge_applies()` instead, which composes `marker_matches` with
/// extras-aware logic specific to dep edges.
///
/// Returns `true` if the marker is absent (universal applicability).
pub fn marker_matches(marker: Option<&MarkerTree>, env: &MarkerEnvironment) -> bool {
    // ... existing body unchanged ...
}
```

- [ ] **Step 2: Update `reachable_with_extras` doc comment in `src/lock/graph.rs`**

Find the existing function and replace/extend its doc comment:

```rust
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
pub fn reachable_with_extras(/* ... */) {
    // ... existing body unchanged ...
}
```

- [ ] **Step 3: Verify compile + tests pass**

Run: `cargo test`
Expected: PASS (only doc comments changed).

- [ ] **Step 4: Commit**

```bash
git add src/platform.rs src/lock/graph.rs
git commit -m "docs(s2): clarify marker_matches role and extras asymmetry

marker_matches now documented as the canonical non-edge marker
evaluator (edge_applies is for dep-edges). reachable_with_extras
spells out the global/include_groups vs target-side/DepEdge.extra
asymmetry with a worked example.

Closes TECH_DEBT items: 'marker_matches in src/platform.rs is dead
code'; 'pep508_rs extras activation asymmetry needs a louder comment'."
```

---

## Phase 9 — Close out

### Task 22: Update `TECH_DEBT.md`

**Files:**
- Modify: `docs/superpowers/TECH_DEBT.md`

- [ ] **Step 1: Move all closed items to the Resolved section**

For each of the eight items closed in Tasks 1–6, 19, 20, 21 above, move the entry from `## Open` to `## Resolved` and append the closing commit SHA. Use `git log --oneline -20` to find the right SHAs.

The eight items to move:
1. Cycle error formatting is opaque (Task 1)
2. Fixture 06-cycle-error assertion is too loose (Task 2)
3. print_deps.rs uses `std::env::current_dir()` (Task 3)
4. derive_env_strings warning branch is unreachable (Task 6)
5. marker_matches in src/platform.rs is dead code (Task 21)
6. pep508_rs extras activation asymmetry needs a louder comment (Task 21)
7. Fixture goldens are version-pinned to current PyPI (Task 19)
8. include_groups duplicates are silently preserved (Task 20)

Format for the Resolved section entries:

```markdown
### Cycle error formatting is opaque
- **Resolved:** S2, commit `<sha>`
- **Summary:** `LockfileError::Cycle` storage changed to `Vec<Vec<String>>`
  with a hand-rolled multi-line `Display`. `detect_cycles` walks SCCs in
  edge-following order, rotated to start at the lex-smallest member.
```

(One entry per closed item; bullet points fit on three lines each.)

- [ ] **Step 2: Verify the Open section is now smaller**

Open should retain: BadVersion → BadUrl split (S4); BadGroupName regex (S6); Tarjan SCC iterative (S4+); expand_requires_python floor (any); RawConfig::platforms #[serde(default)] (any).

- [ ] **Step 3: Commit**

```bash
git add docs/superpowers/TECH_DEBT.md
git commit -m "docs: move 8 S2-resolved tech-debt items to Resolved section

Closing entries reference the implementing commits. Open section
retains the items deferred to later stages (S4, S6, +)."
```

---

### Task 23: Update roadmap

**Files:**
- Modify: `docs/superpowers/specs/2026-05-20-muntjac-roadmap.md`

- [ ] **Step 1: Mark S2 ✅ shipped in the specs index table**

Find the line:

```
| S2 | (not yet written) | (not yet written) | ⬜ next |
```

Replace with:

```
| S2 | [2026-05-20-muntjac-s2-wheel-selector-design.md](./2026-05-20-muntjac-s2-wheel-selector-design.md) | [2026-05-20-muntjac-s2-wheel-selector.md](../plans/2026-05-20-muntjac-s2-wheel-selector.md) | ✅ shipped (tag `s2-complete`, <N> commits, <T> tests) |
```

Replace `<N>` with the commit count and `<T>` with `cargo test 2>&1 | tail -5` total. Update the next row's status to ⬜ next as well:

```
| S3 | (not yet written) | (not yet written) | ⬜ next |
```

- [ ] **Step 2: Tag the stage**

Run: `git tag s2-complete`

- [ ] **Step 3: Commit**

```bash
git add docs/superpowers/specs/2026-05-20-muntjac-roadmap.md
git commit -m "docs: mark S2 shipped in roadmap index

S2 (platform model & wheel selector) shipped. Tagged s2-complete.
S3 (first BUCK emitter) is the next stage."
```

---

## Verification gate

Before declaring S2 complete, run:

```bash
cargo test                                 # all tests pass
cargo clippy --all-targets -- -D warnings  # no warnings
cargo fmt --check                          # formatting clean
```

Then verify the exit criteria from spec §12 by hand:

- `muntjac debug pick-wheels` produces JSON shape ✓ (Task 16, fixture 04 test)
- `01-numpy-matrix` golden passes byte-stable ✓ (Task 17)
- Determinism: byte-identical two runs ✓ (Task 18, fixture 05)
- Strict baseline validation errors are exact ✓ (Task 4 tests)
- All 8 tech-debt items closed ✓ (Task 22 entries)

Tests expected: ~103 total (S1's 68 + ~35 added in S2).
