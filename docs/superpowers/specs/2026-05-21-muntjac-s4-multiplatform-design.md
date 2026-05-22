# Muntjac S4 — Multi-platform + multi-python BUCK ("numpy day one")

**Status:** draft v1 (2026-05-21)
**Companion to:** [`2026-05-20-muntjac-design.md`](./2026-05-20-muntjac-design.md), [`2026-05-20-muntjac-roadmap.md`](./2026-05-20-muntjac-roadmap.md)
**Position:** stage S4 of Phase 1
**Depends on:** S0 (scaffolding), S1 (lockfile parser & graph), S2 (wheel selector), S3 (first BUCK emitter) — all shipped
**Feeds into:** S5 (sdist prebake)

---

## 1. Scope

S4 promotes the S3 emitter from a single-platform × N-pythons matrix to **M-platforms × N-pythons**, adds a generated `wiring.bzl` helper that wires the host OS+CPU axis into muntjac's per-cell constraints (loaded once from the user's root PACKAGE), and gates completion on an end-to-end CI smoke that actually builds and runs a numpy-importing Python binary under `buck2` on three runners.

**In scope**

- **Emitter extension:** S3's cross-cell dep-set mismatch error is dropped; the emitter now renders `deps = select({...})` when uv.lock reports cell-varying deps. Uniform cells continue to render plain lists.
- **Wiring helper:** generated `<third_party_dir>/wiring.bzl` exports a top-level constant `MUNTJAC_HOST_MODIFIERS` (a list of nested `ModifiersMatch` dicts) binding host OS+CPU → muntjac's `linux-x86_64-gnu` / `linux-aarch64-gnu` / `macos-arm64` constraint values. Users load the constant from their **root** PACKAGE and pass it to `set_cfg_modifiers(cfg_modifiers = MUNTJAC_HOST_MODIFIERS)`; users also call `set_cfg_constructor(...)` directly. (The prelude blocks `set_cfg_modifiers` from being called inside any `.bzl` file via a `call_stack_frame` check in `set_cfg_modifiers.bzl` — so muntjac can't wrap the calls in a `setup_muntjac()` function.) Python version remains a user-side modifier (per-binary `modifiers = [...]` attr or a root-PACKAGE default), documented in the generated `muntjac.bzl` header. Muntjac does NOT emit a `<third_party_dir>/PACKAGE` (PACKAGE modifiers don't propagate to deps from a sibling package, so emitting one there would be misleading dead weight — validated by the Phase-1 spike).
- **Fixtures:** new `02-numpy-pandas` (numpy + transitive deps, 3 platforms × 2 pythons = 6 cells; roadmap-fixed name) and `03-musllinux` (emit-only golden compare of a musllinux-only wheel pick). `01-pure-python` gets its goldens regenerated for the new file set (PACKAGE dropped, wiring.bzl added, visibility=PUBLIC added, native.* form for native rules).
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

Host-platform-axis wiring is exported from a generated `<third_party_dir>/wiring.bzl` as a top-level constant `MUNTJAC_HOST_MODIFIERS = [<nested ModifiersMatch dict>]`. Users load the constant from their root PACKAGE and call `set_cfg_modifiers(cfg_modifiers = MUNTJAC_HOST_MODIFIERS)` directly (plus a one-time `set_cfg_constructor(...)` registration). The prelude blocks wrapping `set_cfg_modifiers` in a `.bzl` function, so the user owns the actual call site. Python-version-axis wiring stays user-side (per-binary `modifiers = [...]` attribute or a root-PACKAGE default) — matching reindeer's convention for the rust-toolchain axis and respecting mainline buck2's absence of a per-target `python_version` attribute on `python_binary`.

Muntjac models reindeer's split: it owns the third-party dep rules and their wiring helper, but **does not** generate `.buckconfig`, prelude submodule references, `toolchains/BUCK`, or the user's root PACKAGE. Those are project scaffolding the user provides once. (A future `muntjac init` may scaffold templates, but that's S8+ work, not S4.)

CI gains a buck2 install step + a `buck2 run //tests/smoke:numpy_demo` step on each of three runners. The smoke's `demo.py` does an in-process shape assertion (`assert arr.shape == (3,)`) so a wrong-cell match surfaces as a non-zero exit, not a silent green. CI also installs a cp312 interpreter and ensures `python3.12` is on PATH for the prelude's system_python_toolchain to find.

---

## 3. Module layout

S4 doesn't add new modules. It extends existing ones:

```
src/buck/emit.rs              — EmitDeps enum (new); build_emit_input() bail!() dropped
src/buck/string_writer.rs     — multi-cell select() rendering for deps; wiring.bzl
                                rendering; muntjac.bzl uses native.* for native rules;
                                config/BUCK gains visibility=["PUBLIC"]
src/buck/write.rs             — writes wiring.bzl instead of PACKAGE
src/lock/parser.rs            — BadUrl variant migration
src/lock/graph.rs             — strongconnect rewritten iteratively
.github/workflows/ci.yml      — buck2 install + cp312 install + smoke steps
tests/fixtures/buck/02-numpy-pandas/  — new (incl. hand-written root PACKAGE +
                                        toolchains/BUCK consumed by smoke)
tests/fixtures/buck/03-musllinux/     — new
tests/fixtures/buck/01-pure-python/expected/  — regenerated goldens
                                                (drops PACKAGE; adds wiring.bzl;
                                                adds visibility=PUBLIC; muntjac.bzl
                                                switches to native.* for native rules)
docs/superpowers/specs/2026-05-20-muntjac-design.md  — §1 free-threaded non-goal removed
docs/superpowers/TECH_DEBT.md  — three items moved to Resolved; cp313t retargeted
```

The `EmitOutput` type's `package_file: String` field is renamed to `wiring_bzl: String`. The file written to disk is `wiring.bzl`, not `PACKAGE`.

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

The macro signature stays `pypi_package(name, version, wheels, deps = [], visibility = None, **kwargs)`. Buck handles `select()` values transparently when passed via `deps =` because `prebuilt_python_library` accepts selects.

**Critical S3-bug fix**: `http_file`, `prebuilt_python_library`, and `alias` are native rules in the buck2 prelude — they have no Starlark `load()` paths. From inside a `.bzl` file they must be invoked as `native.<rule>`. S3's emitter used bare names with phantom `load()` calls, which would have failed under buck2 (S3's emitter was never tested end-to-end with `buck2 build`). S4 corrects this for both the S3 fixture (01-pure-python golden regen) and new fixtures.

S4 also adds a header comment documenting the python-axis wiring contract and the wiring.bzl setup:

```python
##
## @generated by muntjac
##
## Wiring contract for consumers:
##
##   One-time project setup — add to your root PACKAGE:
##
##     load("//<third_party_dir>:wiring.bzl", "setup_muntjac")
##     setup_muntjac()
##
##   This registers buck2's cfg_constructor and auto-routes the host
##   OS+CPU to the matching muntjac platform constraint.
##
##   To pick a python version, set a per-binary modifier or a
##   root-PACKAGE default:
##
##     # per-binary
##     python_binary(
##         name = "service",
##         modifiers = ["//<third_party_dir>/config:py312"],
##         deps = ["//<third_party_dir>:numpy"],
##         main = "main.py",
##     )
##
##     # root PACKAGE default (set once for the whole project)
##     load("@prelude//cfg/modifier:set_cfg_modifiers.bzl", "set_cfg_modifiers")
##     set_cfg_modifiers(["//<third_party_dir>/config:py312"])
##
## Available muntjac python constraints: py311, py312
##

_CONFIGS = [
    "py311-linux-aarch64-gnu",
    "py311-linux-x86_64-gnu",
    "py311-macos-arm64",
    "py312-linux-aarch64-gnu",
    "py312-linux-x86_64-gnu",
    "py312-macos-arm64",
]

def pypi_package(name, version, wheels, deps = [], visibility = None, **kwargs):
    unknown = [cfg for cfg in wheels.keys() if cfg not in _CONFIGS]
    if unknown:
        fail("unknown config(s) in {}: {}".format(name, unknown))

    for cfg, wheel in wheels.items():
        url, sha = wheel
        if sha.startswith("sha256:"):
            sha = sha[len("sha256:"):]
        wheel_name = "{}-{}-{}-wheel".format(name, version, cfg)
        native.http_file(
            name = wheel_name,
            sha256 = sha,
            urls = [url],
            visibility = [],
        )
        native.prebuilt_python_library(
            name = "{}-{}__{}".format(name, version, cfg),
            binary_src = ":" + wheel_name,
            deps = deps,
            visibility = [],
        )

    native.alias(
        name = "{}-{}".format(name, version),
        actual = select({
            "//<third_party_dir>/config:" + cfg: ":{}-{}__{}".format(name, version, cfg)
            for cfg in wheels.keys()
        }),
        visibility = [],
    )
    native.alias(
        name = name,
        actual = ":{}-{}".format(name, version),
        visibility = visibility if visibility != None else ["PUBLIC"],
    )
```

The available-constraints list in the header reflects the tree's actual `python_versions`. Implementer renders it dynamically. Note: no `load()` for `http_file`/`prebuilt_python_library`/`alias`/`expect` — none are loadable Starlark symbols in this prelude; the `expect(...)` form is replaced with `if unknown: fail(...)` for clarity.

### `<third_party_dir>/wiring.bzl` (NEW in S4)

Exports a top-level constant `MUNTJAC_HOST_MODIFIERS` — a list of nested `ModifiersMatch` dicts. The user loads it from their **root** PACKAGE and calls `set_cfg_modifiers(cfg_modifiers = MUNTJAC_HOST_MODIFIERS)` directly. The user also registers buck2's `cfg_constructor` (the open-source prelude doesn't do this by default).

```python
##
## @generated by muntjac
## Do not edit by hand.
##
## Host-axis modifiers for muntjac's per-cell platform constraints.
##
## The user's root PACKAGE loads MUNTJAC_HOST_MODIFIERS and passes it
## to set_cfg_modifiers() directly. set_cfg_modifiers() cannot be wrapped
## in a helper function — the prelude enforces that it be called from
## a PACKAGE/BUCK_TREE file, not a .bzl file.
##
## Wiring contract for consumers — add to your root PACKAGE:
##
##   load(
##       "@prelude//cfg/modifier:cfg_constructor.bzl",
##       "cfg_constructor_post_constraint_analysis",
##       "cfg_constructor_pre_constraint_analysis",
##   )
##   load("@prelude//cfg/modifier:common.bzl", "MODIFIER_METADATA_KEY")
##   load("@prelude//cfg/modifier:set_cfg_modifiers.bzl", "set_cfg_modifiers")
##   load("//<third_party_dir>:wiring.bzl", "MUNTJAC_HOST_MODIFIERS")
##
##   set_cfg_constructor(
##       stage0 = cfg_constructor_pre_constraint_analysis,
##       stage1 = cfg_constructor_post_constraint_analysis,
##       key = MODIFIER_METADATA_KEY,
##       aliases = struct(),
##       extra_data = struct(),
##   )
##   set_cfg_modifiers(cfg_modifiers = MUNTJAC_HOST_MODIFIERS)

MUNTJAC_HOST_MODIFIERS = [
    {
        "_type": "ModifiersMatch",
        "prelude//os/constraints:linux": {
            "_type": "ModifiersMatch",
            "prelude//cpu/constraints:arm64":  "root//<third_party_dir>/config:linux-aarch64-gnu",
            "prelude//cpu/constraints:x86_64": "root//<third_party_dir>/config:linux-x86_64-gnu",
        },
        "prelude//os/constraints:macos": {
            "_type": "ModifiersMatch",
            "prelude//cpu/constraints:arm64":  "root//<third_party_dir>/config:macos-arm64",
        },
    },
]
```

Validated against buck2 `2026-05-18` + prelude SHA `b4e55417b...` by the Phase-1 spike. Key shape constraints (all enforced by the prelude's `verify_normalized_modifier` / `is_modifiers_match` checks):

- The `ModifiersMatch` dict requires an explicit `"_type": "ModifiersMatch"` discriminator key.
- All target labels must be **fully qualified** (`root//...`, `prelude//...`) — bare `//path:name` is rejected.
- OS/CPU constraint setting targets are `prelude//os/constraints:os` and `prelude//cpu/constraints:cpu`; constraint values live under the same package as the setting.
- The prelude lacks an `aarch64` CPU constraint — `aarch64` linux maps to `prelude//cpu/constraints:arm64` (same target macOS arm64 uses).
- Nested `ModifiersMatch` dicts are supported.
- `set_cfg_modifiers` is a PACKAGE-only primitive: it checks `call_stack_frame(1).module_path` and `fail()`s if the caller isn't a PACKAGE/BUCK_TREE file. Wrapping it in a `.bzl` function is rejected — so muntjac exports the data (a constant), not the call.

The `<third_party_dir>` placeholder is replaced at emit time with the actual configured directory. Inner-dict keys + values reflect only the platforms declared in the user's `muntjac.toml` (musllinux platforms don't get a host-axis branch since the open-source prelude has no `musl` OS constraint; users targeting musl pick a cell explicitly via `--modifier` or a config_setting). When EVERY platform is musllinux, `MUNTJAC_HOST_MODIFIERS = []` (an empty list).

### `<third_party_dir>/PACKAGE` — NOT EMITTED

S4 does not emit a `<third_party_dir>/PACKAGE`. PACKAGE-level modifiers attach to targets *defined* under that PACKAGE; they do not propagate to deps when those deps are configured from a consumer in a sibling package. Emitting `set_cfg_modifiers` there would be misleading dead weight. The wiring.bzl approach (above) puts the modifiers at the root PACKAGE where they actually affect dep resolution.

### `<third_party_dir>/config/BUCK`

Extended from S3 to declare both axes' constraint_values and all cell config_settings. All constraint_values and config_settings carry `visibility = ["PUBLIC"]` because the root PACKAGE references them via a different cell-relative path (`root//<third_party_dir>/config:foo` from the project root) and resolution fails without PUBLIC visibility. For the 6-cell case:

```python
##
## @generated by muntjac
## Do not edit by hand.
##

constraint_setting(name = "python_version")
constraint_value(name = "py311", constraint_setting = ":python_version", visibility = ["PUBLIC"])
constraint_value(name = "py312", constraint_setting = ":python_version", visibility = ["PUBLIC"])

constraint_setting(name = "platform")
constraint_value(name = "linux-aarch64-gnu", constraint_setting = ":platform", visibility = ["PUBLIC"])
constraint_value(name = "linux-x86_64-gnu",  constraint_setting = ":platform", visibility = ["PUBLIC"])
constraint_value(name = "macos-arm64",       constraint_setting = ":platform", visibility = ["PUBLIC"])

config_setting(
    name = "py311-linux-aarch64-gnu",
    constraint_values = [":linux-aarch64-gnu", ":py311"],
    visibility = ["PUBLIC"],
)
config_setting(
    name = "py311-linux-x86_64-gnu",
    constraint_values = [":linux-x86_64-gnu", ":py311"],
    visibility = ["PUBLIC"],
)
config_setting(
    name = "py311-macos-arm64",
    constraint_values = [":macos-arm64", ":py311"],
    visibility = ["PUBLIC"],
)
# ... three more for py312, sorted lex
```

constraint_values inside each `config_setting`'s `constraint_values` list are sorted lex (platform before python_version alphabetically). config_settings are sorted lex by name. constraint_values within each axis are sorted lex. `constraint_setting` itself doesn't need a `visibility` attr — it's only referenced via `constraint_value(constraint_setting = ...)` from within the same package.

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
├── .buckconfig           declares prelude cell + cell aliases
├── .gitmodules           (root-level; prelude submodule pointer lives here)
├── prelude/              git submodule (gitignored ./prelude/ content but the pointer
│                         lives in .gitmodules at repo root)
├── PACKAGE               hand-written root PACKAGE; loads + calls setup_muntjac()
│                         (worked example of muntjac's user-side setup)
├── toolchains/
│   └── BUCK              hand-written system_python + system_cxx toolchains
├── expected/
│   ├── BUCK              one pypi_package call per resolved package
│   ├── muntjac.bzl       6-cell _CONFIGS + native.* rules + python-axis header
│   ├── wiring.bzl        setup_muntjac() with cfg_constructor + host-axis modifiers
│   └── config/BUCK       2 py-axis + 3 plat-axis constraint_values, 6 config_settings,
│                         all visibility = ["PUBLIC"]
└── tests/smoke/
    ├── BUCK              python_binary(name="numpy_demo", modifiers=[...], deps=[":numpy"])
    └── demo.py           import numpy as np; arr=np.zeros(3); print(arr); assert arr.shape==(3,)
```

The fixture root is a buck2 cell. `.buckconfig`, `PACKAGE`, `toolchains/BUCK`, and `tests/smoke/` are hand-written user-side artifacts (committed to demonstrate the setup). `expected/` holds golden output from `muntjac buckify` and gets byte-compared in the integration test. The generated `third-party/python/` directory (containing the live `BUCK` + `muntjac.bzl` + `wiring.bzl` + `config/BUCK`) is gitignored — it's regenerated by buckify and the goldens are the source of truth for snapshots.

Note that the **fixture's** `PACKAGE` (hand-written, committed) is at `02-numpy-pandas/PACKAGE` — the project root. There is intentionally NO `02-numpy-pandas/third-party/python/PACKAGE` in the emitted output set.

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
    ├── wiring.bzl
    └── config/BUCK
```

Note: no hand-written `.buckconfig`, root PACKAGE, or `tests/smoke/` for 03 — this fixture is emit-only (no `buck2 build` step in CI; Linux GH runners are glibc, not musl). The `expected/wiring.bzl` for the musl-only platform set contains a `set_cfg_modifiers` call whose host-axis dict has no `prelude//os/constraints:linux` branch matching a musl variant (musl is not a first-class OS constraint in the open-source prelude); musllinux platforms therefore require user-side modifier selection at build time rather than auto-routing.

Initial package candidate: `psycopg2-binary` (publishes both manylinux and musllinux variants; muntjac's `linux-x86_64-musl` cell forces musllinux selection). The planner can substitute a different package if `psycopg2-binary` becomes impractical (uv.lock churn, transitive sdists). The fixture's purpose is the selector exercise, not the specific package.

### `tests/fixtures/buck/10-determinism/` (carryover)

Continues unchanged. May be re-pointed at `02-numpy-pandas` or remain on `01-pure-python` — either exercises the determinism machinery equivalently.

### `tests/fixtures/buck/01-pure-python/` (regenerated goldens)

S4 doesn't change the *inputs* but the *outputs* shift in three ways: (1) the S3 PACKAGE placeholder is dropped from the emitted file set, (2) a new `wiring.bzl` is added (single-platform — the host-axis modifier set has one branch), and (3) `config/BUCK` gains `visibility = ["PUBLIC"]` on every entry plus any platform-axis declarations that weren't in S3. Additionally, `muntjac.bzl` shifts from S3's bare-name native-rule calls to `native.<rule>` form — the spike showed the S3 muntjac.bzl would not have loaded under buck2.

Reviewer eyeballs the diff on the PR; the determinism test catches any new HashMap iteration sneaking in.

### `tests/fixtures/buck/README.md` addendum

Append a paragraph: fixtures `02-*` and later exercise a (N platforms × M pythons) cell matrix; uv.lock and goldens are jointly frozen — regeneration touches both in one commit.

---

## 7. CI workflow

### New steps in `.github/workflows/ci.yml`

Added after the existing `cargo test` step on all three matrix runners:

```yaml
- uses: actions/checkout@v4
  with:
    submodules: recursive    # fetches the prelude submodule used by the fixture

- name: install cp312 python
  # Required by the prelude's system_python_toolchain. Both `uv python install`
  # and apt/brew work; uv is most consistent across runners.
  run: |
    pipx install uv || python3 -m pip install --user uv
    uv python install 3.12
    # Ensure `python3.12` resolves on PATH (system_python_toolchain looks
    # up the bare command unless the toolchain target overrides the path).
    UV_PY_BIN="$(uv python find 3.12)"
    sudo ln -sf "$UV_PY_BIN" /usr/local/bin/python3.12
    python3.12 --version

- name: install buck2 (pinned)
  run: |
    BUCK2_RELEASE="2026-05-18"
    case "${{ matrix.runner }}" in
      ubuntu-latest)    ASSET="buck2-x86_64-unknown-linux-gnu.zst" ;;
      ubuntu-24.04-arm) ASSET="buck2-aarch64-unknown-linux-gnu.zst" ;;
      macos-latest)     ASSET="buck2-aarch64-apple-darwin.zst" ;;
      *) echo "unknown runner ${{ matrix.runner }}"; exit 1 ;;
    esac
    mkdir -p "$HOME/.local/bin"
    curl -L "https://github.com/facebook/buck2/releases/download/${BUCK2_RELEASE}/${ASSET}" -o /tmp/buck2.zst
    if ! command -v zstd >/dev/null 2>&1; then
      if [ "${{ runner.os }}" = "macOS" ]; then brew install zstd; else sudo apt-get update && sudo apt-get install -y zstd; fi
    fi
    zstd -d /tmp/buck2.zst -o "$HOME/.local/bin/buck2"
    chmod +x "$HOME/.local/bin/buck2"
    echo "$HOME/.local/bin" >> "$GITHUB_PATH"
    "$HOME/.local/bin/buck2" --version

- name: muntjac buckify (02-numpy-pandas fixture)
  run: |
    cd tests/fixtures/buck/02-numpy-pandas
    cargo run --release --manifest-path ../../../../Cargo.toml -- buckify
    test -f third-party/python/BUCK
    test -f third-party/python/muntjac.bzl
    test -f third-party/python/wiring.bzl
    test -f third-party/python/config/BUCK

- name: buck2 build + run numpy demo
  run: |
    cd tests/fixtures/buck/02-numpy-pandas
    buck2 run //tests/smoke:numpy_demo 2>&1 | tee /tmp/numpy-out.txt
    grep -F "[0. 0. 0.]" /tmp/numpy-out.txt
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

## 10. Implementation order (spike done; results folded into §5)

The cfg-modifier API spike was completed in the first execution pass and validated against buck2 `2026-05-18` + prelude SHA `b4e55417b4edf582be8fb20f24e1afc5866987ce`. Findings are folded into §5 above (the file-contents section) and into the §6 fixture description. The spike surfaced five contract-affecting deviations from the original draft (PACKAGE wiring location, `set_cfg_constructor` registration, `_type` discriminator, fully-qualified targets, `native.*` for native rules); this spec revision is post-spike.

Remaining work, in order:

1. **Emitter implementation**: code the emitter to produce the validated §5 shapes.
2. **Goldens**: freeze `expected/*` files based on emitter output.
3. **Per-cell deps machinery**: independent; parallel-eligible.
4. **Tech-debt fold-ins** (BadUrl, iterative Tarjan): independent; parallel-eligible.
5. **CI workflow**: add the steps from §7.
6. **Doc edits**: design spec §1 non-goal removal, TECH_DEBT shuffles, roadmap update.

The CI smoke is the forcing function — we cannot ship green CI without the wiring being correct end-to-end.

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
- Generated `<third_party_dir>/wiring.bzl` exports a top-level constant `MUNTJAC_HOST_MODIFIERS` (a list of nested `ModifiersMatch` dicts with `"_type"` discriminators + fully-qualified `root//...` constraint targets). Users `load` it from their root PACKAGE and pass to `set_cfg_modifiers` directly (the prelude blocks wrapping the call in a `.bzl` function).
- Generated `<third_party_dir>/PACKAGE` is **not** in the emitted file set.
- Generated `<third_party_dir>/muntjac.bzl` uses `native.<rule>` for the native rules (`http_file`, `prebuilt_python_library`, `alias`) and contains the python-axis wiring header pointing at both the per-binary `modifiers = [...]` snippet and the `setup_muntjac()` invocation.
- Generated `<third_party_dir>/config/BUCK` declares `visibility = ["PUBLIC"]` on every `constraint_value` and `config_setting`.
- `tests/smoke/BUCK` + `tests/smoke/demo.py` committed inside the `02-numpy-pandas` fixture as the canonical worked example. The fixture also commits a hand-written root `PACKAGE` (loading `setup_muntjac`) and `toolchains/BUCK` (system python + cxx) to demonstrate the user-side setup.

### Folded-in tech debt (all moved to `TECH_DEBT.md ## Resolved`)

- `LockfileError::BadUrl` variant exists; sdist/wheel/git URL parse failures migrated off `BadVersion`.
- `strongconnect` in `src/lock/graph.rs` is iterative; existing graph tests pass unchanged.
- Main design spec §1 free-threaded non-goal removed; `cp313t` TECH_DEBT entry retargeted to "S5 or later" (not moved to Resolved — code work is still deferred).

### Demo

`cargo install muntjac && cd 02-numpy-pandas-fixture && muntjac buckify && buck2 run //tests/smoke:numpy_demo` prints `[0. 0. 0.]` on Linux x86_64, Linux aarch64, and macOS arm64. The roadmap's "real `import numpy as np; print(np.zeros(3))` Python binary built by Buck against muntjac-generated rules" demo is live.

---

## 13. Risks & mitigations

- **cfg-modifier API churn (validated).** Spike validated against buck2 `2026-05-18` + prelude SHA `b4e55417...`. CI pins the same buck2 version so the validated shape stays valid. Future prelude releases may break the wiring; if so, CI smoke surfaces it as a red build and we bump the pin + adjust the emitter together.

- **buck2 install action stability in CI.** No officially-maintained GitHub Action for buck2 install exists. Mitigation: pin buck2 version, install via `curl + zstd` from the release artifact. Step is required (no silent skip). If the binary release moves, the install step errors and CI is red until the URL is fixed.

- **Open-source prelude's `set_cfg_constructor` isn't registered by default.** Without explicit registration, `set_cfg_modifiers` and per-target `modifiers` attrs are silently no-ops. The spike caught this and the emitter's `wiring.bzl::setup_muntjac()` registers the constructor explicitly. New users following the wiring contract get it for free; users who skip `setup_muntjac()` get a silent miss. Mitigated by clear docs in `muntjac.bzl` header + the fixture root PACKAGE as a worked example.

- **musllinux fixture package-availability drift.** `psycopg2-binary`'s musllinux variants may change in future releases. Mitigation: uv.lock is frozen in the fixture; golden BUCK references the frozen URL. If the package becomes impractical (e.g. all native deps), planner picks an alternative — the fixture's purpose is the *selector exercise*, not the specific package.

- **Numpy 2.x wheel availability across all 6 cells.** Verified at spec-writing time; numpy 2.1+ publishes cp311/cp312 wheels for manylinux x86_64, manylinux aarch64, and macOS arm64. Risk is low but non-zero. Mitigation: planner verifies again when freezing the fixture's uv.lock; if a cell lacks a wheel, the fixture's `python_versions` or `platforms` is trimmed accordingly with a note in the fixture README.

- **Regenerating `01-pure-python` goldens.** A new PACKAGE shape means S3's goldens drift. Mitigation: golden regen happens in the same commit as the emitter change; reviewer reads the diff to confirm only intended bytes moved. Determinism test runs against the new goldens too.

- **Per-cell `deps = select({...})` becoming noisy in pathological lockfiles.** A lockfile with many marker-gated deps could produce select() everywhere. Mitigation: not a blocker for S4 (numpy's tree has none); if a future user reports unwieldy output, the emitter could grow a heuristic for "collapse when N-1 of N cells agree." Filed as a future-work note in TECH_DEBT only if a real user reports it.

---

## 14. Open questions

None blocking. Deviations during implementation should be filed as follow-ups in `TECH_DEBT.md`.
