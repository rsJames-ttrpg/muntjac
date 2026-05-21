# Muntjac S4 — Multi-platform + multi-python BUCK ("numpy day one")

**Status:** draft v1 (2026-05-21)
**Companion to:** [`2026-05-20-muntjac-design.md`](./2026-05-20-muntjac-design.md), [`2026-05-20-muntjac-roadmap.md`](./2026-05-20-muntjac-roadmap.md)
**Position:** stage S4 of Phase 1
**Depends on:** S0 (scaffolding), S1 (lockfile parser & graph), S2 (wheel selector), S3 (first BUCK emitter) — all shipped
**Feeds into:** S5 (sdist prebake)

---

## 1. Scope

S4 promotes the S3 emitter from a single-platform × N-pythons matrix to **M-platforms × N-pythons**, makes the generated `PACKAGE` file a real artifact (auto-wiring host OS+CPU onto muntjac's per-cell constraints), and gates completion on an end-to-end CI smoke that actually builds and runs a numpy-importing Python binary under `buck2` on three runners.

**In scope**

- **Emitter extension:** S3's cross-cell dep-set mismatch error is dropped; the emitter now renders `deps = select({...})` when uv.lock reports cell-varying deps. Uniform cells continue to render plain lists.
- **PACKAGE wiring:** generated `<third_party_dir>/PACKAGE` calls `set_cfg_modifiers` with an inline `ModifiersMatch` dict that binds host OS+CPU → muntjac's `linux-x86_64-gnu` / `linux-aarch64-gnu` / `macos-arm64` constraint values. Python version remains a user-side modifier (per-binary `modifiers = [...]` attr or a root-PACKAGE default), documented in the generated `muntjac.bzl` header.
- **Fixtures:** new `02-numpy-pandas` (numpy + transitive deps, 3 platforms × 2 pythons = 6 cells; roadmap-fixed name) and `03-musllinux` (emit-only golden compare of a musllinux-only wheel pick). `01-pure-python` gets its goldens regenerated for the new PACKAGE bytes.
- **CI e2e smoke:** new `tests/fixtures/buck/02-numpy-pandas/tests/smoke/` directory with a hand-written `BUCK` + `demo.py` running `import numpy as np; print(np.zeros(3))`. CI installs buck2 and runs `buck2 run //tests/smoke:numpy_demo` on `ubuntu-latest`, `ubuntu-24.04-arm`, and `macos-latest`.
- **Folded-in tech debt:** `LockfileError::BadUrl` variant + URL-parse-failure migration; iterative Tarjan SCC in `src/lock/graph.rs::strongconnect`; main design spec §1 edit dropping the free-threaded Python non-goal (no code; TECH_DEBT entry retargeted to S5+).

**Out of scope**

- Native sdists — still error with the S3 "no wheel + no native-sdist support" message → S5.
- Local fixups / community registry — S6 / S7.
- Vendor mode — S9.
- A `muntjac_python_binary` macro wrapper — explicitly rejected in §3 in favor of documented `modifiers = [...]` snippets.
- Free-threaded Python (`cp313t`) implementation — design-spec non-goal lifted in S4 (a doc-only change), but code retargeted to S5 or later per the updated TECH_DEBT entry.
- Pandas, scipy, torch in the fixture set — defer to S8 launch-demo time.
- musllinux running under `buck2 build` — Linux GH runners are glibc; the `03-musllinux` fixture is emit-only.

---

## 2. Approach

The S3 emitter pipeline already produces a `BTreeMap<ConfigName, EmitWheel>` per package; S4 generalizes the **deps** side of `EmitPackage` from `Vec<String>` to an enum that can express either uniform-across-cells deps or per-cell deps. The emitter at render time chooses between `deps = [...]` and `deps = select({...})` based on the variant.

Host-platform-axis wiring is added to the generated PACKAGE via Buck2's `set_cfg_modifiers` machinery (which is real and stable in the prelude). Python-version-axis wiring is documented as a user-side `modifiers = [...]` attribute or a root-PACKAGE default — matching reindeer's convention for the rust-toolchain axis and respecting mainline buck2's absence of a per-target `python_version` attribute on `python_binary`.

CI gains a buck2 install step + a `buck2 run //tests/smoke:numpy_demo` step on each of three runners. The smoke's `demo.py` does an in-process shape assertion (`assert arr.shape == (3,)`) so a wrong-cell match surfaces as a non-zero exit, not a silent green.

---

## 3. Module layout

S4 doesn't add new modules. It extends existing ones:

```
src/buck/emit.rs              — EmitDeps enum (new); build_emit_input() bail!() dropped
src/buck/string_writer.rs     — multi-cell select() rendering for deps; PACKAGE auto-wiring
src/lock/parser.rs            — BadUrl variant migration
src/lock/graph.rs             — strongconnect rewritten iteratively
.github/workflows/ci.yml      — buck2 install + smoke steps
tests/fixtures/buck/02-numpy-pandas/  — new
tests/fixtures/buck/03-musllinux/     — new
tests/fixtures/buck/01-pure-python/expected/  — regenerated goldens
docs/superpowers/specs/2026-05-20-muntjac-design.md  — §1 free-threaded non-goal removed
docs/superpowers/TECH_DEBT.md  — three items moved to Resolved; cp313t retargeted
```

---

## 4. Type model

### `EmitDeps` enum (new in `src/buck/emit.rs`)

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EmitDeps {
    /// Every cell has the same dep list. Rendered as `deps = [":foo", ":bar"]`.
    Uniform(Vec<String>),

    /// Cells differ. Rendered as `deps = select({...})` with one branch per cell.
    /// Keys are every cell in `EmitInput::configs`; no `default` arm because
    /// the cell coverage is exhaustive by construction.
    PerCell(BTreeMap<ConfigName, Vec<String>>),
}
```

`EmitPackage::deps` changes type:

```rust
pub struct EmitPackage {
    pub name: String,
    pub version: String,
    pub deps: EmitDeps,  // was: Vec<String>
    pub wheels: BTreeMap<ConfigName, EmitWheel>,
}
```

### Collapse rule (in `build_emit_input`)

After populating `pkg_deps_per_cell: BTreeMap<PkgKey, BTreeMap<ConfigName, Vec<String>>>`:

```rust
let cell_deps: BTreeMap<ConfigName, Vec<String>> = /* per-cell deps */;
let mut iter = cell_deps.values();
let first = iter.next().expect("at least one cell");
let uniform = iter.all(|v| v == first);
let deps = if uniform {
    EmitDeps::Uniform(first.clone())
} else {
    EmitDeps::PerCell(cell_deps)
};
```

The S3 bail!() at `src/buck/emit.rs:184` is deleted.

---

## 5. Generated file contents

### `<third_party_dir>/BUCK`

**Uniform-deps `pypi_package` call (unchanged from S3):**

```python
pypi_package(
    name = "requests",
    version = "2.32.3",
    deps = [":certifi", ":charset-normalizer", ":idna", ":urllib3"],
    wheels = { "py311-linux-x86_64-gnu": (...), ... },
    visibility = ["PUBLIC"],
)
```

**Per-cell-deps `pypi_package` call (new in S4):**

```python
pypi_package(
    name = "rich",
    version = "13.7.1",
    deps = select({
        "//third-party/python/config:py311-linux-aarch64-gnu": [":markdown-it-py", ":pygments", ":typing-extensions"],
        "//third-party/python/config:py311-linux-x86_64-gnu":  [":markdown-it-py", ":pygments", ":typing-extensions"],
        "//third-party/python/config:py311-macos-arm64":       [":markdown-it-py", ":pygments", ":typing-extensions"],
        "//third-party/python/config:py312-linux-aarch64-gnu": [":markdown-it-py", ":pygments"],
        "//third-party/python/config:py312-linux-x86_64-gnu":  [":markdown-it-py", ":pygments"],
        "//third-party/python/config:py312-macos-arm64":       [":markdown-it-py", ":pygments"],
    }),
    wheels = { ... },
    visibility = ["PUBLIC"],
)
```

select() branches are sorted by `ConfigName` lex order. The dict has one entry per cell in `EmitInput::configs` (no default arm). The third_party_dir placeholder is replaced with the configured value at emit time.

### `<third_party_dir>/muntjac.bzl`

The macro signature stays `pypi_package(name, version, wheels, deps = [], visibility = None, **kwargs)`. Buck handles `select()` values transparently when passed via `deps =` because `prebuilt_python_library` accepts selects. The `expect(set(wheels.keys()).issubset(set(_CONFIGS)))` invariant is unchanged.

S4 adds a header comment documenting the python-axis wiring contract:

```python
##
## @generated by muntjac
##
## Wiring contract for consumers:
##
##   Host OS + CPU is auto-wired in <third_party_dir>/PACKAGE. To pick a
##   python version, set a per-binary modifier or a root-PACKAGE default:
##
##     # per-binary
##     python_binary(
##         name = "service",
##         modifiers = ["//<third_party_dir>/config:py312"],
##         deps = ["//<third_party_dir>:numpy"],
##         main = "main.py",
##     )
##
##     # root PACKAGE default
##     load("@prelude//cfg/modifier:set_cfg_modifiers.bzl", "set_cfg_modifiers")
##     set_cfg_modifiers(["//<third_party_dir>/config:py312"])
##
## Available muntjac python constraints: py311, py312
##

load("@prelude//python:python_library.bzl", "prebuilt_python_library")
load("@prelude//utils:utils.bzl", "expect")

_CONFIGS = [
    "py311-linux-aarch64-gnu",
    "py311-linux-x86_64-gnu",
    "py311-macos-arm64",
    "py312-linux-aarch64-gnu",
    "py312-linux-x86_64-gnu",
    "py312-macos-arm64",
]

def pypi_package(name, version, wheels, deps = [], visibility = None, **kwargs):
    # ... body unchanged from S3
```

The available-constraints list in the header reflects the tree's actual `python_versions`. Implementer renders it dynamically.

### `<third_party_dir>/PACKAGE`

```python
##
## @generated by muntjac
## Do not edit by hand.
##

load("@prelude//cfg/modifier:set_cfg_modifiers.bzl", "set_cfg_modifiers")

# Bind the build's host OS+CPU to muntjac's platform constraint.
# Python version is picked by the user via a per-binary `modifiers` attr
# or a root PACKAGE default; see muntjac.bzl header for the snippet.

set_cfg_modifiers(
    cfg_modifiers = [
        {
            "config//os/constraints:linux": {
                "config//cpu/constraints:x86_64": "//<third_party_dir>/config:linux-x86_64-gnu",
                "config//cpu/constraints:arm64":  "//<third_party_dir>/config:linux-aarch64-gnu",
            },
            "config//os/constraints:macos": {
                "config//cpu/constraints:arm64":  "//<third_party_dir>/config:macos-arm64",
            },
        },
    ],
)
```

The `config//os/constraints:*` and `config//cpu/constraints:*` references are the open-source-prelude convention. **The exact cell prefix (`config//` vs `prelude//` vs install-dependent) is verified by the implementation spike (§10); the spec commits to the *shape* — a nested `ModifiersMatch` dict that distinguishes the three host triples in our matrix.**

Outer dict keys are config_settings matched against the build's current configuration; inner values are constraint_value targets to be set. The S3-era "placeholder PACKAGE" disclaimer is removed.

### `<third_party_dir>/config/BUCK`

Extended from S3 to declare both axes' constraint_values and all cell config_settings. For the 6-cell case:

```python
##
## @generated by muntjac
## Do not edit by hand.
##

constraint_setting(name = "python_version")
constraint_value(name = "py311", constraint_setting = ":python_version")
constraint_value(name = "py312", constraint_setting = ":python_version")

constraint_setting(name = "platform")
constraint_value(name = "linux-aarch64-gnu", constraint_setting = ":platform")
constraint_value(name = "linux-x86_64-gnu",  constraint_setting = ":platform")
constraint_value(name = "macos-arm64",       constraint_setting = ":platform")

config_setting(
    name = "py311-linux-aarch64-gnu",
    constraint_values = [":linux-aarch64-gnu", ":py311"],
)
config_setting(
    name = "py311-linux-x86_64-gnu",
    constraint_values = [":linux-x86_64-gnu", ":py311"],
)
config_setting(
    name = "py311-macos-arm64",
    constraint_values = [":macos-arm64", ":py311"],
)
# ... three more for py312, sorted lex
```

constraint_values inside each `config_setting`'s `constraint_values` list are sorted lex (platform before python_version alphabetically). config_settings are sorted lex by name. constraint_values within each axis are sorted lex.

---

## 6. Fixtures

### `tests/fixtures/buck/02-numpy-pandas/`

Roadmap-fixed name; scope is **numpy + its transitive deps only**, 3 platforms × 2 pythons = 6 cells. Pandas is deferred to S8 launch demo to keep CI fast.

```
tests/fixtures/buck/02-numpy-pandas/
├── muntjac.toml          platforms = [linux-x86_64-gnu, linux-aarch64-gnu, macos-arm64]
│                         python_versions = ["3.11", "3.12"]
├── pyproject.toml        single dep: numpy>=2.1,<2.3
├── uv.lock               frozen; numpy 2.x ships wheels for all 6 cells
├── expected/
│   ├── BUCK              one pypi_package call per resolved package
│   ├── muntjac.bzl       6-cell _CONFIGS list + python-axis wiring header
│   ├── PACKAGE           host-axis set_cfg_modifiers wiring
│   └── config/BUCK       2 py-axis + 3 plat-axis constraint_values, 6 config_settings
├── tests/smoke/
│   ├── BUCK              python_binary(name="numpy_demo", modifiers=[...], deps=[":numpy"])
│   └── demo.py           import numpy as np; arr=np.zeros(3); print(arr); assert arr.shape==(3,)
└── .buckconfig           declares prelude cell + any required cell aliases (config//, etc.)
```

The fixture root is a buck2 cell — `02-numpy-pandas/` carries a `.buckconfig` so `buck2 run //tests/smoke:numpy_demo` resolves correctly. The exact `.buckconfig` contents (prelude path, cell aliases for `config//`, python_toolchain registration) are produced by the §10 spike — they're whatever it takes to make `buck2 run` green against the pinned buck2 version. If the prelude needs to be checked out into the fixture (via git submodule, fetched in CI, or vendored), that's planner choice.

Numpy 2.1+ has no runtime dependencies and ships cp311/cp312 wheels for all three target platforms. No NoWheel cell and no marker-gated deps, so this fixture exercises the uniform-deps path. The per-cell `select()`-deps machinery is verified in unit tests, not this fixture.

### `tests/fixtures/buck/03-musllinux/`

Emit-only verification — no `buck2 build` (Linux GH runners are glibc).

```
tests/fixtures/buck/03-musllinux/
├── muntjac.toml          platforms = [linux-x86_64-musl]; python_versions = ["3.12"]
├── pyproject.toml        depends on a package with musllinux-only wheels for this cell
├── uv.lock               frozen
└── expected/
    ├── BUCK              wheel URL contains *musllinux_1_2_x86_64*
    ├── muntjac.bzl
    ├── config/BUCK
    └── PACKAGE
```

Initial package candidate: `psycopg2-binary` (publishes both manylinux and musllinux variants; muntjac's `linux-x86_64-musl` cell forces musllinux selection). The planner can substitute a different package if `psycopg2-binary` becomes impractical (uv.lock churn, transitive sdists). The fixture's purpose is the selector exercise, not the specific package.

### `tests/fixtures/buck/10-determinism/` (carryover)

Continues unchanged. May be re-pointed at `02-numpy-pandas` or remain on `01-pure-python` — either exercises the determinism machinery equivalently.

### `tests/fixtures/buck/01-pure-python/` (regenerated goldens)

S4 doesn't change the *inputs* but the *outputs* shift because PACKAGE goes from placeholder to real auto-wiring, and `config/BUCK` may gain the host-axis constraint_values it didn't declare in S3. The regenerated goldens land in the same commit as the emitter change. Reviewer eyeballs the diff on the PR; the determinism test catches any new HashMap iteration sneaking in.

### `tests/fixtures/buck/README.md` addendum

Append a paragraph: fixtures `02-*` and later exercise a (N platforms × M pythons) cell matrix; uv.lock and goldens are jointly frozen — regeneration touches both in one commit.

---

## 7. CI workflow

### New steps in `.github/workflows/ci.yml`

Added after the existing `cargo test` step on all three matrix runners:

```yaml
- name: install buck2
  # exact action / install path verified by the spike; one of:
  #   - dedicated buck2 install action if one is maintained
  #   - cargo install buck2 (slow but reliable)
  #   - curl + tar from facebook/buck2's release artifacts
  # planner picks the most stable option at implementation time.
  run: |
    curl -L https://github.com/facebook/buck2/releases/download/<pinned-version>/buck2-<host-triple>.zst | \
        zstd -d -o /usr/local/bin/buck2
    chmod +x /usr/local/bin/buck2
    buck2 --version

- name: muntjac buckify (02-numpy-pandas fixture)
  run: |
    cd tests/fixtures/buck/02-numpy-pandas
    cargo run --release -- buckify

- name: buck2 build + run numpy demo
  run: |
    cd tests/fixtures/buck/02-numpy-pandas
    buck2 run //tests/smoke:numpy_demo
```

`buck2 run` exits non-zero if `demo.py`'s `assert arr.shape == (3,)` fails or if any earlier step errors. Each runner naturally picks its own cell from the alias-with-select:

- `ubuntu-latest` → `py312-linux-x86_64-gnu`
- `ubuntu-24.04-arm` → `py312-linux-aarch64-gnu`
- `macos-latest` → `py312-macos-arm64`

That's the structural shape of the three-runner demo: each runner proves its own cell works end-to-end.

### Buck2 version pinning

The buck2 version is pinned in the workflow. Bumping it is an explicit PR. Rationale: the cfg-modifier prelude API is in churn — pin the version where we know the wiring works.

### No silent skip

If `buck2 install` fails (upstream release moves, mirror down), the step is marked **required**; CI is red, not yellow. We don't let "buck2 unavailable, skipping" rot into a permanent skip.

### Existing test suite unchanged

`cargo fmt --check`, `cargo clippy`, `cargo build`, `cargo test` all keep running on every PR on all three runners. The buck2 smoke is additive.

---

## 8. Errors

### Changed: dep-set cross-cell mismatch is no longer an error

The S3 `bail!()` at `src/buck/emit.rs:184` is deleted. Cell-varying deps produce `EmitDeps::PerCell` (rendered as `select()`).

### Unchanged: NoWheel still errors

The S3 NoWheel error message (precise `(package, version, py, platform)` tuple, "S5 lands" hint) stays exactly as is. S5 replaces it with sdist prebake.

### Folded-in: `LockfileError::BadVersion` → `BadUrl` migration

New error variant in `src/error.rs` (or wherever `LockfileError` lives):

```rust
#[error("bad URL for package '{package}' field '{field}': {url} ({reason})")]
BadUrl {
    package: String,
    field: &'static str,   // "sdist", "wheel", "git source"
    url: String,
    reason: String,
},
```

`src/lock/parser.rs` migrates every `LockfileError::BadVersion { reason: "wheel URL: ..." }` (and the sdist / git variants) to `BadUrl { package, field, url, reason }` with the appropriate static `field`. Tests asserting old error strings get updated; ~3-4 tests touched.

### Folded-in: iterative Tarjan SCC

`src/lock/graph.rs::strongconnect` becomes iterative using an explicit `Vec<Frame>` work stack. Standard textbook iterative Tarjan — each frame carries the node, an iterator over outgoing edges, and the running `lowlink`. Algorithm output is identical; only the stack management changes. Existing `graph.rs::tests` cycle/SCC tests pass unchanged.

### What S4 does NOT add as errors

- No new validation for user-side `python_binary` mis-configuration (missing `modifiers` attr produces a buck2 select-miss error at build time — that's the right surface).
- No CI fallback / retry on buck2 install failures (see §7).

---

## 9. Testing

### Unit tests, `src/buck/emit.rs`

- **`EmitDeps::Uniform` collapse** — synthetic 6-cell input with identical dep lists; assert result is `Uniform`.
- **`EmitDeps::PerCell` preservation** — synthetic 6-cell input where two cells differ; assert result is `PerCell` with all 6 cells present.
- **Single-cell collapse** — 1-cell input always collapses to `Uniform`.
- **Rename + flip** of the existing S3 `build_emit_input_errors_on_dep_set_cross_cell_mismatch` test: previously-erroring input now produces `PerCell`; assert variant + contents.

### Unit tests, `src/buck/string_writer.rs`

- **Uniform-deps rendering** — plain `deps = [":foo", ":bar"]` form.
- **Per-cell-deps rendering** — `deps = select({...})` form, branches sorted by `ConfigName` lex.
- **6-cell `_CONFIGS`** — snapshot asserts the list of 6 strings.
- **PACKAGE host-axis wiring** — snapshot of the `set_cfg_modifiers` call's nested `ModifiersMatch` dict.
- **`config/BUCK` multi-platform** — snapshot covering 2 py-axis + 3 plat-axis constraint_values, 6 config_settings, all sorted.

### Integration: `tests/buckify.rs`

- **`fixture_02_numpy_pandas_golden`** — copy fixture into tempdir, run `muntjac buckify`, byte-compare every output file against `expected/`.
- **`fixture_03_musllinux_golden`** — same shape; additionally assert the wheel URL field contains `*musllinux*`.
- **`fixture_01_pure_python_golden`** — passes after goldens are regenerated for the new PACKAGE bytes.
- **`fixture_10_determinism`** — unchanged.

### CI smoke (covered in §7)

The `buck2 run //tests/smoke:numpy_demo` step is the e2e gate. The `assert arr.shape == (3,)` inside `demo.py` is the actual smoke assertion.

### Tech-debt fold-in tests

- **`BadUrl` migration** — `parser.rs` tests asserting URL-parse-error messages get updated. ~3-4 tests touched, no net new tests.
- **Iterative Tarjan SCC** — existing `graph.rs::tests` cycle/SCC tests are the regression net. Optional: a synthetic deep-chain stress test (~1000 nodes) demonstrating the no-stack-overflow property.

### Test budget

| Source | Tests |
|---|---|
| S3 baseline | 145 |
| New emit.rs unit tests (deps enum) | +4 |
| New string_writer.rs unit tests | +5 |
| Fixture 02-numpy-pandas + 03-musllinux | +2 |
| BadUrl test updates | 0 net |
| Tarjan stress test (optional) | +0 or +1 |
| **Total** | **~156** |

---

## 10. Implementation order (de-risks cfg-modifier API uncertainty)

The exact cfg-modifier API surface — specifically the `config//os/constraints:*` cell prefix, whether nested `ModifiersMatch` dicts work as drawn, the precise constraint-setting targets for OS+CPU — is in churn upstream. The plan reorders tasks to validate against real buck2 BEFORE goldens are frozen:

1. **Spike** (first task): install the pinned buck2 in a scratch checkout, set up `02-numpy-pandas/.buckconfig` (and any prelude bootstrap), write a minimal PACKAGE + config/BUCK + BUCK by hand matching the §5 shapes, run `buck2 run //tests/smoke:numpy_demo`. Iterate until exit-0 and `[0. 0. 0.]` on the configured runner. Output of the spike: a known-good hand-written fixture + a buck2 install procedure that the CI workflow can copy verbatim.
2. **Emitter implementation**: code the emitter to produce the shape that was just validated by the spike.
3. **Goldens**: freeze `expected/*` files based on emitter output (which now matches the hand-validated shape).
4. **Per-cell deps machinery**: independent of the API spike; can land in parallel.
5. **Tech-debt fold-ins** (BadUrl, iterative Tarjan): independent; parallel.

If the spike surfaces a contract-breaking gap (e.g. nested `ModifiersMatch` dicts aren't supported and we have to fall back to multiple flat dicts in `cfg_modifiers = [d1, d2, d3]`), the spec is amended before the plan continues. The CI smoke is the safety net regardless.

---

## 11. Folded-in design-spec edit

`docs/superpowers/specs/2026-05-20-muntjac-design.md` §1 currently lists as a non-goal:

> **Free-threaded Python (PEP 703).** Tags like `cp3Xt` are handled safely by selector (refused to match), but no explicit support.

S4 removes this line. The corresponding TECH_DEBT entry (`cp313t free-threaded ABI not first-class`) is retargeted from "S4+" to "S5 or later" — implementation is still deferred, but the project no longer documents free-threaded as out of scope for v1.

The edit is part of S4's commit set; it's a doc-only change (no code) but it's S4's responsibility to make it so the project's posture stays consistent.

---

## 12. Exit criteria

S4 ships when all of the following hold green on CI's three matrix runners (`ubuntu-latest`, `ubuntu-24.04-arm`, `macos-latest`).

### Roadmap-mandated

- `muntjac buckify` on the `02-numpy-pandas` fixture produces byte-identical artifacts matching `expected/`.
- `buck2 run //tests/smoke:numpy_demo` succeeds on all three runners, printing `[0. 0. 0.]` and exiting 0 via the in-`demo.py` shape assertion.
- `03-musllinux` fixture buckifies; the golden BUCK contains a `*musllinux*` substring in the picked wheel's URL field.

### S4-specific additions

- `EmitDeps::PerCell` rendered as `deps = select({...})` when cells diverge; uniform cells continue to render plain lists.
- Generated `PACKAGE` auto-wires the host OS+CPU axis via `set_cfg_modifiers` with inline `ModifiersMatch` dicts (or equivalent shape if the spike requires variation).
- Generated `muntjac.bzl` header documents the per-binary `modifiers = [...]` snippet and the root-PACKAGE-default snippet for the python-version axis.
- `tests/smoke/BUCK` + `tests/smoke/demo.py` committed inside the `02-numpy-pandas` fixture as the canonical worked example.

### Folded-in tech debt (all moved to `TECH_DEBT.md ## Resolved`)

- `LockfileError::BadUrl` variant exists; sdist/wheel/git URL parse failures migrated off `BadVersion`.
- `strongconnect` in `src/lock/graph.rs` is iterative; existing graph tests pass unchanged.
- Main design spec §1 free-threaded non-goal removed; `cp313t` TECH_DEBT entry retargeted to "S5 or later" (not moved to Resolved — code work is still deferred).

### Demo

`cargo install muntjac && cd 02-numpy-pandas-fixture && muntjac buckify && buck2 run //tests/smoke:numpy_demo` prints `[0. 0. 0.]` on Linux x86_64, Linux aarch64, and macOS arm64. The roadmap's "real `import numpy as np; print(np.zeros(3))` Python binary built by Buck against muntjac-generated rules" demo is live.

---

## 13. Risks & mitigations

- **cfg-modifier API churn.** The buck2 prelude's modifier surface (constraint-setting cell prefix, ModifiersMatch nesting) is evolving. Mitigation: spike task in §10 validates against the pinned buck2 BEFORE goldens are frozen; CI smoke is the forcing function — we cannot ship green CI without a working wiring.

- **buck2 install action stability in CI.** No officially-maintained GitHub Action for buck2 install exists as of writing. Mitigation: pin buck2 version, install via `curl + zstd` from the release artifact, falling back to `cargo install buck2` if the binary release moves. Step is required (no silent skip).

- **musllinux fixture package-availability drift.** `psycopg2-binary`'s musllinux variants may change in future releases. Mitigation: uv.lock is frozen in the fixture; golden BUCK references the frozen URL. If the package becomes impractical (e.g. all native deps), planner picks an alternative — the fixture's purpose is the *selector exercise*, not the specific package.

- **Numpy 2.x wheel availability across all 6 cells.** Verified at spec-writing time; numpy 2.1+ publishes cp311/cp312 wheels for manylinux x86_64, manylinux aarch64, and macOS arm64. Risk is low but non-zero. Mitigation: planner verifies again when freezing the fixture's uv.lock; if a cell lacks a wheel, the fixture's `python_versions` or `platforms` is trimmed accordingly with a note in the fixture README.

- **Regenerating `01-pure-python` goldens.** A new PACKAGE shape means S3's goldens drift. Mitigation: golden regen happens in the same commit as the emitter change; reviewer reads the diff to confirm only intended bytes moved. Determinism test runs against the new goldens too.

- **Per-cell `deps = select({...})` becoming noisy in pathological lockfiles.** A lockfile with many marker-gated deps could produce select() everywhere. Mitigation: not a blocker for S4 (numpy's tree has none); if a future user reports unwieldy output, the emitter could grow a heuristic for "collapse when N-1 of N cells agree." Filed as a future-work note in TECH_DEBT only if a real user reports it.

---

## 14. Open questions

None blocking. Deviations during implementation should be filed as follow-ups in `TECH_DEBT.md`.
