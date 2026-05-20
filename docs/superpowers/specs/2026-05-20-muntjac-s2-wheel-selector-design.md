# Muntjac S2 — Platform model & wheel selector

**Status:** draft v1 (2026-05-20)
**Companion to:** [`2026-05-20-muntjac-design.md`](./2026-05-20-muntjac-design.md), [`2026-05-20-muntjac-roadmap.md`](./2026-05-20-muntjac-roadmap.md)
**Position:** stage S2 of Phase 1
**Depends on:** S0 (scaffolding), S1 (lockfile parser & graph) — both shipped
**Feeds into:** S3 (first BUCK emitter)

---

## 1. Scope

S2 turns muntjac's wheel data from "opaque filename strings on `Wheel` objects" into "the right wheel picked deterministically for every `(package, version, platform, python_version)` cell."

**In scope**

- A `src/wheel/` module with three files: filename/tag parsing (PEP 425), platform-tag construction (PEP 600 manylinux + PEP 656 musllinux + macOS deployment-target rules), and the wheel selector itself.
- Tighter `Platform` baseline validation in `src/config.rs`. The existing `manylinux`/`musllinux`/`macos_min` fields are currently inert; S2 makes them load-bearing.
- A hidden `muntjac debug pick-wheels` subcommand that emits JSON describing each cell's chosen wheel or `NoWheel` outcome.
- Folded-in tech debt: cycle-error formatting + fixture 06 tightening + `globals.workdir` plumbing (Important); `marker_matches`/`derive_env_strings`/extras-asymmetry cleanups (Polish); fixture conventions README + `include_groups` dedup (Housekeeping).

**Out of scope**

- BUCK emission — S3 territory.
- Sdist classification, prebake, native-sdist error path — S5 territory. S2's selector reports `NoWheel`; no error is raised at this stage.
- Wheel URL fetching, vendoring, hash verification — S5/S9.
- Fixup application — S6/S7.
- PyPy / GraalPy / Jython tag handling. CPython-only this stage; non-CPython tags are silently classified as `Other(String)` and never appear in any compatible list. Adding PyPy support later is a localized change.
- Wheel discovery beyond what's declared in `uv.lock`. The selector consumes only the wheel list uv already resolved.

---

## 2. Approach

The selector uses the **compatible-tag list** algorithm — the same model pip and uv use internally. For each `(Platform, PythonVersion)`, muntjac constructs an ordered list of compatible PEP 425 tags, most-preferred first. For each candidate wheel, muntjac parses its filename (expanding compressed tag sets), finds the minimum rank across the wheel's tags in the compatible list, and the wheel with the lowest min-rank wins.

This is preferred over a hand-crafted `cmp(a, b)` ordering or a numeric score function because preference is encoded by *construction* — the order of the compatible list — rather than by interpretation. Snapshot-testing the list catches reordering bugs immediately, and "why was this wheel picked" has a one-line answer: "its tag was at rank N in the compatible list."

---

## 3. Module layout

```
src/
  wheel/
    mod.rs          — re-exports {Tag, WheelTag, CompatibleTags, PickResult, pick_wheel}
    tag.rs          — Tag, WheelTag, parse_filename(), compressed-set expansion
    compat.rs       — CompatibleTags, build_compatible_tags(platform, py_version)
    select.rs       — pick_wheel(wheels, &compat) → PickResult
  platform.rs       — existing; gets cleanup tech-debt items
  cli/
    debug/
      pick_wheels.rs   — new subcommand
      print_deps.rs    — existing; gets globals.workdir plumbing
      mod.rs           — extends DebugOp with PickWheels variant
  config.rs         — Platform validation tightened
  error.rs          — LockfileError::Cycle storage shape changes (see §10)
```

Boundaries:

- `tag.rs` knows only how to read a wheel filename. It never looks at a target platform.
- `compat.rs` knows only how to build the ordered list for a target. It never looks at a specific wheel.
- `select.rs` is the only place the two meet. Its job is the lookup-and-rank algorithm.

Each file stays well under 300 lines and is testable in isolation.

---

## 4. Type model

```rust
// src/wheel/tag.rs

/// A single fully-expanded PEP 425 tag triple.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Tag {
    pub python: PythonTag,
    pub abi:    AbiTag,
    pub plat:   PlatformTag,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PythonTag {
    CPython(u8, u8),         // cp312 → CPython(3, 12)
    Py(u8, Option<u8>),       // py3 → Py(3, None);  py37 → Py(3, Some(7))
    Other(String),            // pp310, jy27, ip3 — unsupported, opaque
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum AbiTag {
    CPython(u8, u8),          // cp312
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

#[derive(Debug, Clone, PartialEq, Eq, Hash)] pub enum LinuxArch { X86_64, Aarch64 }
#[derive(Debug, Clone, PartialEq, Eq, Hash)] pub enum MacArch   { X86_64, Arm64, Universal2 }

/// All tags expanded from a wheel filename.
#[derive(Debug, Clone)]
pub struct WheelTag {
    pub tags: Vec<Tag>,         // compressed sets expanded; 1+ entries
    pub raw_filename: String,
}

#[derive(Debug, thiserror::Error)]
pub enum TagParseError {
    #[error("wheel filename `{0}` does not match PEP 427 structure")]
    MalformedFilename(String),
    #[error("unknown structure in tag segment of `{0}`")]
    UnknownTagShape(String),
}
```

```rust
// src/wheel/compat.rs

pub struct CompatibleTags {
    ordered: Vec<Tag>,
    by_tag:  std::collections::HashMap<Tag, usize>,
}

impl CompatibleTags {
    pub fn rank_of(&self, tag: &Tag) -> Option<usize> {
        self.by_tag.get(tag).copied()
    }
    pub fn ordered(&self) -> &[Tag] { &self.ordered }
}

pub fn build_compatible_tags(platform: &Platform, py: PythonVersion) -> CompatibleTags;
```

```rust
// src/wheel/select.rs

#[derive(Debug, Clone)]
pub enum PickResult<'a> {
    Picked {
        wheel: &'a Wheel,
        matched_tag: Tag,
        rank: usize,
    },
    NoWheel,
}

pub fn pick_wheel<'a>(wheels: &'a [Wheel], compat: &CompatibleTags) -> PickResult<'a>;
```

Notes:

- `PlatformTag` carries parsed numeric structure, not strings, so ordering between `manylinux_2_17` and `manylinux_2_28` is type-level.
- `Other(String)` arms keep unsupported tags opaque rather than erroring on PyPy / Jython wheels — they parse, but never appear in any compatible list, so they silently lose.
- `PickResult::Picked` carries `matched_tag` and `rank` so the debug command renders without redoing the lookup.

---

## 5. Filename parsing

Per PEP 427, a wheel filename has shape:

```
{distribution}-{version}(-{build_tag})?-{python_tag}-{abi_tag}-{platform_tag}.whl
```

The three tag segments may each be compressed sets joined by `.`. A wheel matches an env if *any* fully-expanded tag triple matches. Example expansion:

- `numpy-2.1.3-cp312-cp312-manylinux_2_17_x86_64.manylinux2014_x86_64.whl` →
  - `Tag { python: CPython(3,12), abi: CPython(3,12), plat: ManyLinux{2,17,X86_64} }`
  - `Tag { python: CPython(3,12), abi: CPython(3,12), plat: ManyLinux{2,17,X86_64} }` (alias `manylinux2014` ≡ `manylinux_2_17`; deduplicated)

The parser resolves the three PEP 600 / 656 aliases at parse time so downstream code never sees alias forms:

| Source | Canonical |
|---|---|
| `manylinux1_<arch>` | `manylinux_2_5_<arch>` |
| `manylinux2010_<arch>` | `manylinux_2_12_<arch>` |
| `manylinux2014_<arch>` | `manylinux_2_17_<arch>` |

After expansion, duplicates within the same wheel are removed (the canonicalization above can collide). The `WheelTag.tags` vec is order-preserving but deduplicated.

Unparseable filenames raise `TagParseError`. In the selector, parse failures are silently skipped — the wheel never matches anything. (See §9 for why this is preferred over erroring out.)

---

## 6. Compatible-tag construction

For a `(Platform, PythonVersion)`, `build_compatible_tags` emits an ordered list, most-preferred first. The implementation walks the cross product of a **python axis** and a **platform axis**, then concatenates.

### Python axis (example: `python = 3.12`)

In order:

1. `cp312-cp312` — this interpreter, full ABI
2. `cp312-abi3` — this interpreter, stable ABI
3. `cp312-none` — interpreter-specific, no ABI specified
4. `cp311-abi3`, `cp310-abi3`, …, `cp32-abi3` — older stable ABIs (still binary-compatible upward)
5. `py312-none`, `py31-none`, `py3-none`, `py2.py3-none` — interpreter-agnostic, narrowest to widest

PEP 425 says "any older `cp3X-abi3` is compatible with `cp3Y` for `Y >= X`" — that's why item 4 walks back to `cp32-abi3`.

### Platform axis

Depends on `Platform.target` and the parsed baselines from `manylinux`/`musllinux`/`macos_min`:

- **`x86_64-unknown-linux-gnu` with `manylinux = "2_28"`:**
  `manylinux_2_28_x86_64`, `manylinux_2_27_x86_64`, …, `manylinux_2_5_x86_64`, then `any`. Sixteen entries plus `any`. Alias forms (`manylinux2014_x86_64`, etc.) parse to canonical numeric form (§5), so the compatible list contains only canonical entries.

- **`aarch64-unknown-linux-gnu` with `manylinux = "2_28"`:** mirrors gnu-x86_64 with arch `Aarch64`.

- **`x86_64-unknown-linux-musl` with `musllinux = "1_2"`:**
  `musllinux_1_2_x86_64`, `musllinux_1_1_x86_64`, `musllinux_1_0_x86_64`, then `any`.

- **`aarch64-unknown-linux-musl`:** mirrors musl-x86_64 with arch `Aarch64`.

- **`aarch64-apple-darwin` with `macos_min = "11.0"`:**
  For each major in `[MACOS_MAX_MAJOR .. macos_min.major]` descending, emit `macosx_<major>_<minor>_arm64` and `macosx_<major>_<minor>_universal2`. Then `any`. `MACOS_MAX_MAJOR` is a constant (15 as of writing), bumped manually when new macOS major versions ship; living-list approach matches what pip does.

- **`x86_64-apple-darwin`:** mirrors arm64 with `x86_64` and `universal2`. Wheels tagged `arm64` are *not* in this list (and vice versa). `universal2` is in both.

### Combining the axes

The combined list is the cross-product, with the **python axis as the outer loop** (most specific first) and the **platform axis as the inner loop** (most specific first). That is, for each python entry in order, we emit every platform entry in order, before moving to the next python entry.

Concretely: `cp312-cp312-manylinux_2_28_x86_64`, `cp312-cp312-manylinux_2_27_x86_64`, …, `cp312-cp312-any`, then `cp312-abi3-manylinux_2_28_x86_64`, …, eventually ending at `py2.py3-none-any`. So `cp312-cp312-any` beats `py3-none-manylinux_2_28_x86_64` — interpreter specificity dominates platform specificity. This matches pip's resolution order for the same target.

The exact construction is implementation-deterministic and snapshot-tested for every `(platform, python)` pair in the five-platform × three-python matrix (§9).

---

## 7. Selector algorithm

```rust
pub fn pick_wheel<'a>(wheels: &'a [Wheel], compat: &CompatibleTags) -> PickResult<'a> {
    let mut best: Option<(usize, Tag, &Wheel)> = None;

    for wheel in wheels {
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
            }
        }
    }

    match best {
        Some((rank, matched_tag, wheel)) => PickResult::Picked { wheel, matched_tag, rank },
        None => PickResult::NoWheel,
    }
}
```

**Wheel-list normalization:** before iterating, `pick_wheel` sorts the input wheel slice by filename. This removes any dependence on uv.lock's emission order — if uv ever changes how it orders wheels, our tie-break behavior stays stable. Sorting is cheap (typical wheel lists are <30 entries).

**Tie-break for equal ranks:** can't happen in practice — a wheel can only carry each `Tag` once, and a wheel's compressed tag-set expands to distinct `Tag` values. If two *different* wheels in the same package list ever share the best rank, the algorithm picks the first in the normalized (sorted-by-filename) order. A `debug_assertions`-gated assertion fires if this case is ever observed so we notice if uv changes behavior.

**Parse failures:** silently skipped. The debug command's `wheels_considered` field surfaces the count of wheels that *were* parseable so users can distinguish "0 considered, NoWheel" (parser bug) from "12 considered, NoWheel" (real coverage gap).

---

## 8. Config validation tightening

`Config::validate` gains four checks per platform. Existing target-triple allowlist stays.

1. **Linux platforms must declare at least one of `manylinux`, `musllinux`.** Targets ending `linux-gnu` or `linux-musl`. Error: `ConfigError::BadPlatform { name, reason: "linux platform must declare manylinux and/or musllinux baseline" }`.
2. **macOS platforms must declare `macos_min`.** Targets ending `apple-darwin`. Error: `BadPlatform { name, reason: "macOS platform must declare macos_min" }`.
3. **Baselines must parse to canonical form.** `manylinux` and `musllinux` match `^[0-9]+_[0-9]+$`. `macos_min` matches `^[0-9]+\.[0-9]+$`. Aliases like `"2014"` are *not* accepted in config — users write the modern form. Error includes the bad value and the expected shape.
4. **libc coherence.** `musllinux` only on `*-linux-musl` targets, `manylinux` only on `*-linux-gnu` targets. Mixing on a single platform errors. Wanting both means declaring two `[platforms.*]` entries.

Parsed baselines are exposed via accessors:

```rust
impl Platform {
    pub fn manylinux_baseline(&self) -> Option<(u32, u32)>;
    pub fn musllinux_baseline(&self) -> Option<(u32, u32)>;
    pub fn macos_min(&self)            -> Option<(u32, u32)>;
}
```

These re-parse the validated strings every call (parse is cheap) so the `Platform` struct stays serializable as-is. If perf ever matters, the parsed form can move into the struct without API churn.

`Config::validate` running once at `Config::from_str` time guarantees no code path sees an unvalidated `Platform`. Existing unit tests that construct `Platform` literals continue to work since they bypass `from_str`.

**Tied tech-debt item:** `derive_env_strings`'s warning branch in `src/platform.rs` becomes `unreachable!("Config::validate must have rejected this triple")`.

---

## 9. CLI surface

```
muntjac debug pick-wheels [--tree NAME] [--platform NAME] [--python X.Y] [--package NAME]
```

Same filter shape as `print-deps`. With no filters, emits one entry per `(package, version, platform, python_version)` cell that has a non-empty wheel list (i.e. registry and git sources; first-party packages are skipped).

JSON output:

```json
{
  "schema_version": 1,
  "tree": "default",
  "selections": [
    {
      "package": "numpy",
      "version": "2.1.3",
      "platform": "linux-x86_64-gnu",
      "python_version": "3.12",
      "outcome": "picked",
      "wheel": {
        "filename": "numpy-2.1.3-cp312-cp312-manylinux_2_17_x86_64.manylinux2014_x86_64.whl",
        "url":      "https://files.pythonhosted.org/.../numpy-2.1.3-cp312-cp312-manylinux_2_17_x86_64.manylinux2014_x86_64.whl",
        "hash":     "sha256:..."
      },
      "matched_tag": "cp312-cp312-manylinux_2_17_x86_64",
      "rank": 7,
      "wheels_considered": 18
    },
    {
      "package": "native-only-pkg",
      "version": "0.5",
      "platform": "linux-aarch64-gnu",
      "python_version": "3.12",
      "outcome": "no_wheel",
      "wheels_considered": 4,
      "wheels": [
        "native_only_pkg-0.5-cp312-cp312-manylinux_2_17_x86_64.whl",
        "native_only_pkg-0.5-cp311-cp311-manylinux_2_17_x86_64.whl",
        "native_only_pkg-0.5-cp312-cp312-macosx_11_0_arm64.whl",
        "native_only_pkg-0.5-cp311-cp311-macosx_11_0_arm64.whl"
      ]
    }
  ]
}
```

- `outcome: "picked"` carries `wheel`, `matched_tag`, `rank`, `wheels_considered`.
- `outcome: "no_wheel"` carries `wheels_considered` and the *filenames* of considered wheels (not full wheel objects) — keeps JSON compact while still diagnostic.
- `matched_tag` renders in canonical `python-abi-platform` form.
- `schema_version: 1` future-proofs the shape against silent test rot.

Sort order is `(tree, package_name, version, platform_name, python_version)` lexicographic — same key as `print-deps`.

---

## 10. Folded-in tech debt

Each item below becomes a discrete task in the implementation plan. Closing commits move the entry to `TECH_DEBT.md`'s `## Resolved` section with the SHA.

### Important

- **Cycle error formatting** — `src/error.rs`. `LockfileError::Cycle` storage changes from `Vec<String>` to `Vec<Vec<String>>` (outer = cycles, inner = members in actual edge order, rotated to start at lex-smallest member). A hand-rolled `Display` impl emits:
  ```
  dependency cycle(s) detected:
    - alpha@1.0 -> beta@1.0 -> alpha@1.0
    - gamma@2.0 -> delta@2.0 -> gamma@2.0
  ```
  `graph::detect_cycles()` already has the per-SCC member list; the change is in how the data flows to `LockfileError`.

- **Fixture 06 assertion tightening** — `tests/fixtures/lock/06-cycle-error/expected-error.txt` gains a member identifier line, e.g. `- alpha@1.0 -> beta@1.0`. The existing substring assert handles minor whitespace differences.

- **`globals.workdir` plumbing** — new `Globals::workdir() -> PathBuf` returning the absolute resolved value of `-C`. Both `debug print-deps` and `debug pick-wheels` consume it. `cli::run`'s call to `std::env::set_current_dir(path)` is removed. Integration tests use the helper.

### Polish

- **`marker_matches` documentation** — `src/platform.rs`. Doc comment updated to declare it canonical for *non-edge* marker checks. (S2's wheel selector is tag-based, not marker-based, so it doesn't consume `marker_matches` — but future stages may.)

- **`derive_env_strings` unreachable cleanup** — `src/platform.rs`. Warning branch becomes `unreachable!("Config::validate must have rejected this triple")` (per §8).

- **Extras-asymmetry comment** — `src/lock/graph.rs::reachable_with_extras`. Doc comment expanded to spell out the `include_groups` (global, every node) vs `DepEdge.extra` (target-side, per-node) asymmetry with a worked example.

### Housekeeping

- **Fixture conventions README** — new `tests/fixtures/lock/README.md` explaining: lockfiles are frozen artifacts; regenerating requires also regenerating goldens; reviewers should diff both. S2 mirrors this for `tests/fixtures/wheel/README.md`.

- **`include_groups` dedup** — `Config::validate` deduplicates with order preserved and `eprintln!`s a warning if duplicates were removed.

### Deferred (still in `TECH_DEBT.md` Open section)

- `BadVersion` → `BadUrl` variant split (S4)
- `BadGroupName` regex loosening (S6)
- Tarjan SCC iterative form (S4+)
- `expand_requires_python` floor documentation (any)
- `RawConfig::platforms` `#[serde(default)]` (any)

---

## 11. Testing

### Unit tests — `src/wheel/tag.rs`

- Round-trip parse of ~30 representative filenames covering combinations of `cp`/`abi3`/`none`/`py3` × `manylinux`/`musllinux`/`macosx`/`any` × compressed multi-tag.
- Malformed filenames return `TagParseError` (missing fields, bad version digits, unknown structure).
- Compressed-set expansion: `cp310.cp311.cp312-none-any` → 3 distinct `Tag`s.
- Alias canonicalization: `manylinux2014_x86_64` and `manylinux_2_17_x86_64` produce the same canonical `Tag`.

### Unit tests — `src/wheel/compat.rs`

- Snapshot the ordered `CompatibleTags` for each `(platform, py_version)` in a 5-platform × 3-python matrix (15 `insta` snapshots). Stable byte-for-byte across runs.
- Manylinux alias matching: a wheel tagged `manylinux2014_x86_64` is accepted when `manylinux_2_17_x86_64` is in the list, at the same rank.
- musllinux ordering: with `musllinux = "1_2"`, a 1.1 wheel is accepted at lower preference; a 1.1 baseline rejects a 1.2 wheel (`rank_of` returns `None`).
- macOS deployment-target ordering: with `macos_min = "11.0"`, both `macosx_11_0_arm64` and `macosx_14_0_arm64` are accepted, with 14_0 ranked higher.
- x86_64 macOS rejects `arm64` and vice versa; both accept `universal2`.

### Unit tests — `src/wheel/select.rs`

- Canonical scoring order: `cp312-cp312-manylinux_2_17_x86_64` > `cp312-abi3-manylinux_2_17_x86_64` > `py3-none-any`.
- Unparseable filenames are skipped, not errored.
- `NoWheel` outcome when only `cp310` wheels exist for a `cp312` target.
- Tie-break: two wheels with identical best rank → first in input order. Debug assertion fires (gated on `cfg(debug_assertions)`).

### Integration fixtures — `tests/fixtures/wheel/`

- `01-numpy-matrix/` — uv.lock excerpt with real numpy 2.1.3 wheel list. Golden JSON shows every `(platform, py)` cell resolving correctly. This satisfies the roadmap's "numpy 2.1.3 matrix correctly resolves" demo.
- `02-musllinux-only/` — package with only `*musllinux*` tags; matches on `*-linux-musl` platforms, `NoWheel` on `*-linux-gnu`.
- `03-no-wheel/` — package with only `cp310` wheels; `NoWheel` outcome on `cp312` target with non-empty `wheels_considered` (regression coverage for "0 considered" debugging clue).
- `04-pure-python/` — package with only `py3-none-any`; matches every cell.
- `05-determinism/` — runs `pick-wheels` twice on `01-numpy-matrix`, byte-compares output.

### Tests for tech-debt fold-ins

- Cycle error formatting: assert formatted message against fixture 06's expected-error.txt (now containing member identifiers).
- `globals.workdir`: integration test invokes `muntjac -C <fixture-dir> debug pick-wheels` from a different cwd; confirms success without depending on `set_current_dir`.
- `include_groups` dedup: unit test asserts post-parse `Vec` has no duplicates; warning fires.

Test budget estimate: ~35 new tests over S1's 68 → ~103 total passing.

---

## 12. Exit criteria

Stage ships when all of the following hold green in CI on ubuntu-latest, ubuntu-24.04-arm, and macos-latest.

### Roadmap-mandated

- `muntjac debug pick-wheels` reads `uv.lock` + `muntjac.toml`, emits the JSON shape from §9, one entry per `(pkg, ver, platform, py_ver)` cell.
- All unit tests from §11 pass: PEP 425 scoring order, manylinux2014 ↔ manylinux_2_17 aliasing, musllinux 1.1 ↔ 1.2 ordering, macOS deployment-target ordering.
- The `01-numpy-matrix` fixture's `(platform × python_version)` matrix resolves every cell against real numpy 2.1.3 wheel data; golden is byte-stable.

### S2-specific additions

- Determinism: `05-determinism` fixture asserts two invocations produce byte-identical JSON.
- Strict baseline validation: a `muntjac.toml` declaring a linux platform without `manylinux` or `musllinux` produces `BadPlatform` with the exact reason string from §8.
- `manylinux` on a `linux-musl` target (or vice versa) errors with the coherence-check message.

### Folded-in tech debt (all moved to `TECH_DEBT.md ## Resolved`)

- Cycle error renders as the multi-line bulleted format from §10.
- `cli::run` no longer calls `std::env::set_current_dir`; both debug subcommands use `globals.workdir()`.
- `derive_env_strings` warning branch is `unreachable!()`.
- `marker_matches` doc comment explains non-edge use case.
- `reachable_with_extras` doc comment includes the worked asymmetry example.
- `tests/fixtures/lock/README.md` + `tests/fixtures/wheel/README.md` exist.
- `Config::validate` deduplicates `include_groups` with warning.

### Demo

On the `01-numpy-matrix` fixture, `muntjac debug pick-wheels` shows every numpy 2.1.3 wheel correctly resolved for the five-platform × three-python matrix. From a fresh checkout, `cargo test` passes ~103 tests.

---

## 13. Risks & mitigations

- **macOS `MACOS_MAX_MAJOR` constant drifts behind the ecosystem.** A wheel tagged `macosx_16_0_arm64` would be silently rejected if we forget to bump. Mitigation: document the constant prominently with a comment pointing at PyPI's most-tagged macOS version, and add a soft test that the constant is `>= 15` (current ceiling) — failing CI on regression makes the bump intentional.

- **Tag canonicalization collisions surprise users.** A user looking at `manylinux2014_x86_64` in their wheel list and seeing it match a `manylinux_2_17` compatible-list entry might be confused. Mitigation: the debug command's `matched_tag` field renders the canonical form, and we document the alias table in `tag.rs`'s module doc.

- **PEP 425's ABI3 backward-walk rule is subtle.** Walking back from `cp312-abi3` to `cp32-abi3` produces a 12-entry compatible list segment that's mostly noise (no real wheels carry `cp32-abi3`). Mitigation: snapshot tests pin the exact list shape; reviewers see the noise but it's deterministic and matches pip. Premature optimization to trim the walk would diverge from the reference implementation.

- **First-party packages with empty wheel lists shouldn't appear in output.** Filtering them in the debug command is easy to forget. Mitigation: explicit unit test asserts a workspace member with no wheels produces no entries in JSON.

- **uv.lock wheel ordering changes break determinism.** If uv ever sorts wheels differently across versions, our tie-break-by-input-order rule produces different output for the same data. Mitigation: muntjac sorts the wheel list internally by filename before passing to the selector, removing the dependency on uv's emission order. This is a one-line change in `pick_wheels` and doesn't affect correctness (the *best* wheel doesn't depend on input order; only tie-breaks do).

---

## 14. Open questions

None blocking. The design above is committed; deviations during implementation should be filed as follow-ups in `TECH_DEBT.md`.
