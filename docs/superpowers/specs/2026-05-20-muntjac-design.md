# Muntjac Design

**Status:** draft v1 (2026-05-20)
**Author:** Jack Mayo
**One-liner:** Muntjac is `reindeer` for Python — it translates a `uv.lock` into Buck2 build rules so a Buck monorepo can consume PyPI packages without `pip` or `uv` at build time.

---

## 1. Goals & non-goals

### Goals

- **Sharp scope: `uv.lock → BUCK`.** Only uv is supported as the resolver. We do not accept `requirements.txt`, `Pipfile.lock`, `poetry.lock`, or raw `pyproject.toml` without a uv-managed lock. The sharpness is the feature: assuming uv lets us trust the lockfile format, the resolver's output, and the wheel-download semantics.
- **Solve numpy on day one.** The credible v1 launch surface is: numpy, pandas, scipy, torch (CPU), cryptography, requests, fastapi, ruff working end-to-end on Linux x86_64 + macOS arm64. A tool that only handles pure-Python wheels is a toy; a tool that handles the platform-wheel matrix is real infrastructure.
- **Fixups as data.** A community `muntjac-fixups/` registry, layered with per-repo local overrides, is the project's moat. The schema and registry are first-class in v1, not bolted on later.
- **Ship in public early.** Repo open from day one; v0.1.0 announcement on Buck2 Discord, Buck2 GitHub Discussions, and r/Python. Pivot signal: no PR activity on `muntjac-fixups` 90 days post-launch ⇒ rethink.

### Non-goals (v1, explicitly)

- **Windows.** Buck2-on-Windows is immature; wheel-tag matrix complicates further. Clean v2 deliverable.
- **pip / poetry / pipenv / requirements.txt.** uv-only. Migration path: `uv pip compile`.
- **Native sdist Buck-time builds.** When a package has no wheel for a target and is a native sdist, v1 errors with a precise message and an escape-hatch fixup pointing at a hand-rolled Buck rule. The design has hooks for a v2 PEP 517-in-Buck implementation (see §6, §7 `[sdist]`).
- **Editable installs (PEP 660).** Incompatible with Buck's hermetic builds. Permanent non-goal.
- **Free-threaded Python (PEP 703).** Tags like `cp3Xt` are handled safely by selector (refused to match), but no explicit support.
- **Bazel output.** Module structure leaves room for a future emitter under `src/bazel/`; no commitment.
- **Egg-info packages.** Obsolete; uv doesn't emit them.

---

## 2. Architecture overview

Muntjac is a **thin glue over uv**. uv handles resolution, downloads, caching, and sdist→wheel conversion. Muntjac handles:

1. parsing `uv.lock`,
2. selecting the right wheel for each `(platform, python_version)`,
3. classifying sdists as pure-python (pre-bake into a wheel) vs native (error or hand-rolled fixup),
4. applying the layered fixup graph,
5. emitting a deterministic, byte-stable BUCK file plus a small `muntjac.bzl` helper.

Implemented as a single Rust crate with `src/lib.rs` + `src/main.rs` split for internal modularity. No public library API commitment in v1.

External crates pre-chosen: `clap`, `serde`, `toml`/`toml_edit`, `pep440_rs`, `pep508_rs`, `reqwest`, `sha2`, `flate2`, `zip`, `gix`, `insta`, `anyhow`/`thiserror`.

---

## 3. User-facing layout

### Pattern A — monorepo with uv workspace (canonical)

```
repo/
├── pyproject.toml              # uv workspace root; requires-python = ">=3.11,<3.13"
├── uv.lock                     # one resolved lockfile, source of truth for muntjac
├── muntjac.toml                # platforms, python versions, fixup registry pin
├── src/
│   ├── some_python_bin/
│   │   ├── pyproject.toml      # workspace member, declares direct deps
│   │   └── BUCK                # hand-written: python_binary deps = [//third-party/python:numpy]
│   └── another_lib/
└── third-party/python/
    ├── BUCK                    # generated; do not edit
    ├── muntjac.bzl             # generated helper macros
    ├── PACKAGE                 # generated constraint scaffolding
    ├── config/BUCK             # generated config_setting targets
    ├── fixups/                 # local fixup overrides
    │   └── pillow/fixups.toml
    └── vendor/                 # only in --vendor mode
```

### Pattern B — single Python project (no workspace)

Same shape, but `pyproject.toml` + `uv.lock` live with the project rather than at a workspace root. Muntjac doesn't care; it reads paths from `muntjac.toml`.

### `muntjac.toml` (default single-tree shape)

```toml
manifest_path   = "../pyproject.toml"        # workspace root
third_party_dir = "."                         # where BUCK + fixups + vendor land
python_versions = ["3.11", "3.12"]

[platforms]
linux-x86_64-gnu  = { target = "x86_64-unknown-linux-gnu",  manylinux = "2_17" }
linux-aarch64-gnu = { target = "aarch64-unknown-linux-gnu", manylinux = "2_17" }
linux-x86_64-musl = { target = "x86_64-unknown-linux-musl", musllinux = "1_2" }
macos-x86_64      = { target = "x86_64-apple-darwin",       macos_min = "11.0" }
macos-arm64       = { target = "aarch64-apple-darwin",      macos_min = "11.0" }

[fixups]
registry              = "github.com/<user>/muntjac-fixups"
registry_rev          = "<git-sha-or-tag>"
allow_local_overrides = true

[buck]
file_name = "BUCK"
vendor    = false                             # --vendor flips this on
```

### Multi-tree (escape hatch for incompatible dep universes per Python era)

When the same third-party version can't satisfy all first-party packages — e.g. a legacy 3.10 service wants `tensorflow==2.10`, a new 3.12 service wants `tensorflow==2.16` — top-level config keys are replaced by `[tree.<name>]` blocks:

```toml
[platforms] # shared across trees
linux-x86_64-gnu = { target = "x86_64-unknown-linux-gnu", manylinux = "2_17" }
# ...

[fixups]    # shared
registry     = "github.com/<user>/muntjac-fixups"
registry_rev = "abc123..."

[tree.modern]
manifest_path   = "pyproject.toml"
third_party_dir = "third-party/python/modern"
python_versions = ["3.11", "3.12"]

[tree.legacy]
manifest_path   = "legacy/pyproject.toml"
third_party_dir = "third-party/python/legacy"
python_versions = ["3.10"]
```

Each tree is an island; no enforced version coherence between trees. First-party rules pick which tree to depend on by Buck target path.

---

## 4. CLI surface

Match reindeer's verbs to leverage mindshare and reduce naming overhead.

```
muntjac init                       # write starter muntjac.toml, scaffolding
muntjac vendor                     # uv lock (if dirty) + download wheels into ~/.cache/muntjac
                                   #   or ./vendor in --vendor mode
muntjac buckify                    # read uv.lock + fixups; emit BUCK + muntjac.bzl + PACKAGE
muntjac audit                      # cross-check uv.lock vs pypa/advisory-database (OSV)
muntjac fixups update              # refresh registry; bump registry_rev pin; print diff
muntjac fixups show <package>      # print merged effective fixup (community ⊕ local)
muntjac unused                     # report vendored wheels not referenced by any tree
```

### Behavior

- `vendor` re-runs `uv lock` if `pyproject.toml` is newer than `uv.lock`, unless `--frozen`.
- `buckify` is **pure**: same `uv.lock` + same fixup state ⇒ byte-identical output. Sorted iteration; no timestamps; normalized formatting.
- `audit` is non-fatal by default; `--strict` exits non-zero on any finding.
- Global flags: `-C <path>`, `--tree <name>`, `-v`/`-vv`, `--no-network`.
- `--check` on `buckify` (and `vendor` in vendor mode) exits non-zero if on-disk artifacts differ from what would be regenerated. The CI "did someone forget to run muntjac?" gate.

---

## 5. Pipeline

`muntjac buckify` flow:

```
Input: uv.lock + muntjac.toml + fixups/ + registry cache
   ↓
1. Parse uv.lock → Vec<Package { name, version, source, deps, markers, wheels, sdist }>
   ↓
2. Build dep graph
   - resolve env markers per (platform, python_version)
   - detect cycles, dupes
   - drop dev-only deps unless opted in
   ↓
3. Wheel matrix solver
   - for each (package × platform × python_version) pick best wheel by PEP 425 tag score
   - if no wheel → route to step 4
   ↓
4. Sdist classification
   - pure-python sdist → pre-bake via `uv build` at vendor time
   - native sdist → emit error (with fixup escape hatch); v2 emits Buck build rule
   ↓
5. Fixup application
   - load community + local; merge per layering rules (§7)
   - evaluate per-version + per-platform cfg sections
   - produce FixupView per (package, version, platform, python_version)
   ↓
6. BUCK emitter
   - stable ordering, sorted attrs, normalized formatting
   - write BUCK, muntjac.bzl, PACKAGE, config/BUCK
```

### Invariants

- **Determinism.** Same inputs ⇒ byte-identical output. Sort order is lexicographic: (package, version, platform, python_version).
- **Precise errors.** A failure names the exact `(package, version, platform, python_version)` tuple and points to `muntjac fixups show <package>` as the next action.
- **uv shellout points are exactly two:** `uv lock` (when stale) and `uv build` (for pre-baking pure-python sdists). Everything else is native muntjac code.
- **Wheel selection follows PEP 425 + PEP 600** (manylinux) + musllinux PEP 656. Tags are scored by specificity; platform aliases are honored (e.g. a wheel tagged `manylinux2014_x86_64` matches `manylinux_2_17_x86_64`).
- **Sdist classification is conservative.** A sdist is "pure-python" iff `pyproject.toml` declares a `build-backend` in the allowlist (`flit_core`, `hatchling`, `setuptools` without `ext_modules`, `poetry-core`, `pdm-backend`) **and** the source tree has no `setup.py` with `ext_modules`, no `Cargo.toml`, no `meson.build`, no `CMakeLists.txt`, no adjacent `*.c`/`*.cpp`/`*.pyx` sources. Conservative classification produces false negatives (some pure-python sdists treated as native) but no false positives.

---

## 6. BUCK output shape

Two principles:

1. **One semver-stable alias per package.** Users write `//third-party/python:numpy`, not `numpy-2.1.3-cp312-linux_x86_64`.
2. **Selects at the alias level, not inside library rules.** Each (package, version, platform, python_version) gets its own concrete `prebuilt_python_library`. No internal `select()` in the library rule. Easier debugging, better caching.

### Generated `BUCK` (per package, illustrative)

```python
##
## @generated by muntjac
## Do not edit by hand.
##

load("//third-party/python:muntjac.bzl", "pypi_package")

pypi_package(
    name    = "numpy",
    version = "2.1.3",
    deps    = [],
    wheels  = {
        "py311-linux-x86_64-gnu":  ("https://files.pythonhosted.org/.../numpy-2.1.3-cp311-cp311-manylinux_2_17_x86_64.whl",  "sha256:..."),
        "py311-linux-aarch64-gnu": ("https://files.pythonhosted.org/.../numpy-2.1.3-cp311-cp311-manylinux_2_17_aarch64.whl", "sha256:..."),
        "py311-linux-x86_64-musl": ("https://files.pythonhosted.org/.../numpy-2.1.3-cp311-cp311-musllinux_1_2_x86_64.whl",   "sha256:..."),
        "py311-macos-x86_64":      ("https://files.pythonhosted.org/.../numpy-2.1.3-cp311-cp311-macosx_11_0_x86_64.whl",     "sha256:..."),
        "py311-macos-arm64":       ("https://files.pythonhosted.org/.../numpy-2.1.3-cp311-cp311-macosx_11_0_arm64.whl",      "sha256:..."),
        "py312-linux-x86_64-gnu":  ("https://files.pythonhosted.org/.../numpy-2.1.3-cp312-cp312-manylinux_2_17_x86_64.whl",  "sha256:..."),
        # ... etc
    },
    visibility = ["PUBLIC"],
)
```

### Generated `muntjac.bzl` (written once per tree)

```python
##
## @generated by muntjac
##

load("@prelude//python:python_library.bzl", "prebuilt_python_library")
load("@prelude//utils:utils.bzl", "expect")

_CONFIGS = [
    "py311-linux-x86_64-gnu", "py311-linux-aarch64-gnu", "py311-linux-x86_64-musl",
    "py311-macos-x86_64",     "py311-macos-arm64",
    "py312-linux-x86_64-gnu", "py312-linux-aarch64-gnu", "py312-linux-x86_64-musl",
    "py312-macos-x86_64",     "py312-macos-arm64",
]

def pypi_package(name, version, wheels, deps = [], visibility = None, **kwargs):
    expect(set(wheels.keys()).issubset(set(_CONFIGS)),
           "unknown config in {}".format(name))

    for cfg, (url, sha256) in wheels.items():
        http_file(
            name = "{}-{}-{}-wheel".format(name, version, cfg),
            sha256 = sha256.removeprefix("sha256:"),
            urls = [url],
            visibility = [],
        )
        prebuilt_python_library(
            name = "{}-{}__{}".format(name, version, cfg),
            binary_src = ":{}-{}-{}-wheel".format(name, version, cfg),
            deps = deps,
            visibility = [],
        )

    native.alias(
        name = "{}-{}".format(name, version),
        actual = select({
            "//third-party/python/config:{}".format(cfg): ":{}-{}__{}".format(name, version, cfg)
            for cfg in wheels.keys()
        }),
        visibility = [],
    )
    native.alias(
        name = name,
        actual = ":{}-{}".format(name, version),
        visibility = visibility or ["PUBLIC"],
    )
```

### Generated `config/BUCK` and `PACKAGE`

A scaffolding step on `muntjac init` writes `config_setting` targets for each (python_version × platform) combination — one per `_CONFIGS` entry above. PACKAGE wires them via `set_cfg_modifiers` so consumers' `python_binary(python_version=...)` selects the right wheel automatically.

### `--vendor` mode

`http_file(urls=[...])` is replaced by a relative source reference to a wheel committed under `third-party/python/vendor/`. The `prebuilt_python_library` consumers are unchanged; only the wheel-source target swaps. Buck builds become air-gapped.

### Native-sdist path (v2)

A future emitter writes `pypi_sdist_package(...)` calls that generate Buck rules running PEP 517 at build time. The `[sdist]` block in §7 already carries the needed config (backend, build_env, build_deps, extra_native_libs). Until v2, native sdists with no matching wheel emit:

```
error: pillow 11.0.0 has no wheel for (py312, linux-x86_64-musl) and is a native sdist.
       add a fixup at third-party/python/fixups/pillow/fixups.toml — see
       `muntjac fixups show pillow` for the current community fixup, or use
       `replace_deps` to point at a hand-rolled Buck target.
```

### First-party consumer (hand-written)

```python
# src/service/BUCK
python_binary(
    name = "service",
    main = "main.py",
    deps = [
        "//third-party/python:numpy",
        "//third-party/python:fastapi",
    ],
)
```

The `python_binary`'s configured `python_version` flows into the select keys transparently.

---

## 7. Fixup schema & the moat

### File layout

```
third-party/python/fixups/         # local (per-repo)
├── pillow/fixups.toml
├── cryptography/fixups.toml
└── psycopg2/fixups.toml

muntjac-fixups/                    # community registry, separate repo
├── README.md
├── CONTRIBUTING.md
├── packages/
│   ├── numpy/fixups.toml
│   ├── pillow/fixups.toml
│   ├── cryptography/fixups.toml
│   └── ...
└── tests/                          # uv lock → muntjac buckify → buck build smoke
```

### Schema (v1, complete)

```toml
# All top-level keys optional. Apply to all versions × all platforms by default.

# --- Dep overrides ---
extra_deps     = ["//third-party/c:libjpeg"]
omit_deps      = ["typing_extensions"]
replace_deps   = { numpy = "//company/numpy:numpy" }

# --- Wheel selection / patching ---
prefer_wheel   = "sha256:abc..."                # pin specific upload
exclude_wheels = ["*-cp312-*-macosx_*_arm64.*"] # glob over PEP 425 tags
overlay        = "overlay/"                     # dir mirroring wheel root

# --- Sdist build (v2 surface; schema is v1) ---
[sdist]
backend           = "maturin"
build_env         = { MATURIN_PEP517_ARGS = "--release" }
build_deps        = ["//third-party/rust:cargo"]
extra_native_libs = ["//third-party/c:libssl"]
data_files        = ["src/cryptography/.../*.so"]

# --- Entry-points / metadata ---
entry_points = true                             # or ["ruff", "ruff-lsp"]
visibility   = ["//some/path/..."]
labels       = ["security-sensitive"]

# --- Runtime env ---
runtime_env = { LIBJPEG_PATH = "/opt/libjpeg/lib" }

# --- Per-version sections ---
['cfg(version = ">=10.0")']
omit_deps = ["olefile"]

# --- Per-platform sections ---
['cfg(target_os = "linux")']
extra_deps = ["//third-party/c:libssl"]

['cfg(all(target_os = "linux", target_env = "musl"))']
extra_deps = ["//third-party/c:libssl-static"]

# --- Combined cfg ---
['cfg(all(version = ">=42", target_os = "macos"))']
overlay = "overlay-macos-new/"
```

### Layering rules (community ⊕ local)

1. **Load order:** community first, local second. Local overrides.
2. **List-valued fields merge.** `extra_deps`, `omit_deps`, `labels`, `exclude_wheels`, `data_files`, `build_deps` → community ∪ local, deduped, community-first order.
3. **Scalar/dict-valued fields replace.** `overlay`, `prefer_wheel`, `entry_points`, `replace_deps`, `runtime_env`, `[sdist]` subkeys, `visibility` → local wins if set.
4. **Cfg sections evaluate independently** from both layers, applied in order (community → local) with the same merge rule.
5. **Escape hatch:** top-level `replace_community = true` in a local fixup disables the community fixup entirely.

### Cfg predicate grammar

Same expression language as reindeer: `version = "…"`, `target_os = "…"`, `target_arch = "…"`, `target_env = "…"`, plus `all(...)`, `any(...)`, `not(...)`. Cross-pollinating contributors between reindeer and muntjac is the explicit reason for syntactic alignment.

### Registry pinning & updates

- `muntjac.toml` carries `registry_rev = "<git-sha-or-tag>"`. Lockfile-style; no implicit follow-main.
- `muntjac fixups update` bumps to `main` (or `--rev <X>`), prints a structured diff (`+ pillow: new linux-musl variant`, `- olefile fixup removed`), updates the pin.
- Cache at `~/.cache/muntjac/fixups/<sha>/`. Content-addressed; multiple revs coexist.
- **Air-gapped builds:** `[fixups] registry = "none"` uses only local. `registry = "file:///path/to/checkout"` for vendored copy.

### Resolution algorithm

For each `(package_name, version, platform, python_version)`:

```
fixup_view = empty
for layer in [community, local]:
    fixup = layer.load(package_name)
    if fixup is None: continue
    fixup_view ⊕= fixup.top_level
    for section in fixup.cfg_sections:
        if section.predicate.holds_for(version, platform, python_version):
            fixup_view ⊕= section.body
return fixup_view
```

The `FixupView` feeds the BUCK emitter — it carries the final `extra_deps`, `omit_deps`, `overlay`, `entry_points`, etc.

---

## 8. Rust module layout

```
src/
├── main.rs                    # CLI entrypoint, arg parsing
├── lib.rs                     # internal re-exports
├── cli/
│   ├── mod.rs
│   ├── init.rs
│   ├── vendor.rs
│   ├── buckify.rs
│   ├── audit.rs
│   ├── fixups.rs
│   └── unused.rs
├── config.rs                  # muntjac.toml parsing; tree resolution
├── lock/
│   ├── mod.rs
│   ├── parser.rs              # uv.lock TOML → typed
│   └── types.rs               # Package, Wheel, Sdist, Source, Marker, ResolvedDep
├── platform.rs                # platform model, manylinux/musllinux aliasing, marker eval
├── wheel/
│   ├── mod.rs
│   ├── tag.rs                 # PEP 425 tag parsing + scoring
│   └── selector.rs            # (platform, python_ver) → best wheel
├── sdist/
│   ├── mod.rs
│   ├── classifier.rs          # pure-python vs native heuristic
│   └── prebake.rs             # `uv build` shellout
├── fixup/
│   ├── mod.rs
│   ├── schema.rs              # FixupConfig (serde-derived)
│   ├── cfg.rs                 # cfg() predicate parser + evaluator
│   ├── layer.rs               # community ⊕ local merge
│   └── registry.rs            # git fetch + ~/.cache/muntjac/fixups/<sha>
├── buck/
│   ├── mod.rs
│   ├── emit.rs                # main BUCK file emitter
│   ├── bzl.rs                 # writes muntjac.bzl
│   ├── package.rs             # writes PACKAGE + config/BUCK
│   └── format.rs              # canonical formatter
├── uv.rs                      # uv CLI wrappers (lock, build)
├── cache.rs                   # ~/.cache/muntjac/ — registry + downloads
├── advisory.rs                # pypa/advisory-database OSV ingest for `audit`
└── error.rs                   # error types; messages name (pkg, ver, platform, py) tuples
```

### Crate picks

`clap`, `serde`, `toml`/`toml_edit`, `pep440_rs`, `pep508_rs`, `reqwest`, `sha2`, `flate2`, `zip`, `gix` (pure-Rust git), `insta`, `anyhow`/`thiserror`.

---

## 9. Testing

### Unit tests

Heaviest coverage on the algorithmically fiddly modules:

- `wheel/tag.rs` — PEP 425 scoring, manylinux/musllinux aliasing edge cases.
- `fixup/cfg.rs` — predicate parser + evaluator, including version comparison edge cases.
- `fixup/layer.rs` — merge rule correctness (list merge, scalar replace, cfg section ordering).
- `platform.rs` — env marker evaluation; cross-checked against `pep508_rs` reference behavior.

### Snapshot tests (insta)

Each fixture under `tests/fixtures/<scenario>/` contains a minimal `pyproject.toml` + `uv.lock` + optional fixups, plus a golden `BUCK` file. CI fails on byte mismatch.

v1 fixtures:

1. `01-pure-python` — minimal pure-python deps, single platform
2. `02-numpy-pandas` — heavy wheels, manylinux + macOS-arm64
3. `03-musllinux` — alpine target with `*musllinux*`-only wheel
4. `04-local-fixup` — overlay + omit_deps locally
5. `05-community-fixup` — registry layering (registry fixture in-tree)
6. `06-multi-python` — `["3.11", "3.12"]` with env-marker dep diff
7. `07-vendor-mode` — `--vendor` swaps http_file for vendored wheel
8. `08-multi-tree` — two `[tree.X]` blocks producing two BUCK files
9. `09-native-sdist-error` — asserts error message contents
10. `10-determinism` — runs buckify twice, byte-compares

### End-to-end smoke

On Linux x86_64, `buck2 build //third-party/python/...` on the `02-numpy-pandas` fixture must succeed. Skipped when `buck2` not on PATH.

### CI matrix

GitHub Actions:
- `ubuntu-latest` (x86_64)
- `ubuntu-24.04-arm` (aarch64)
- `macos-latest` (arm64)

3 jobs, ~5 min each.

---

## 10. Launch plan

### v0.1.0 cut criteria

End-to-end working on Linux x86_64 + macOS arm64 for: numpy, pandas, fastapi, requests, ruff (entry-point demo). That's the credible launch surface.

### Companion `muntjac-fixups` repo

Seeded at launch with fixups for the platform-quirky tier: `lxml`, `pillow`, `cryptography`, `psycopg2`, `pyzmq`, `opencv-python`, `scipy`, `torch`. CONTRIBUTING.md open from day one.

### Channels

- Buck2 Discord (`#third-party`)
- Buck2 GitHub Discussions
- r/Python weekly tools thread
- Cross-link from reindeer's README/issues if maintainers receptive

### Demo gate (defends against rot)

README's 60-second demo (`cargo install muntjac && muntjac init && uv add numpy && muntjac vendor && muntjac buckify`) is also a CI fixture. Cannot decay silently.

### Pivot signal

No PR activity on `muntjac-fixups` 90 days post-launch ⇒ assumption that this fills a real gap is wrong. Either pivot scope or sunset. Cheap experiment.

---

## 11. Open questions & risks

- **uv version coupling.** uv.lock format and CLI surface are still evolving. Mitigation: pin a tested uv version range in `Cargo.toml`'s `Cargo` runtime check; CI runs against (min-supported, latest) uv. Bump deliberately.
- **Buck2 prelude python rules surface.** `prebuilt_python_library`'s exact API may differ between Buck2 versions. Mitigation: `muntjac.bzl` is generated and lives in the user's repo, so changes can be patched per-repo without a muntjac release.
- **musllinux corpus thinness.** Many native wheels don't publish musllinux variants. Users targeting Alpine will hit "no wheel" errors more often. Mitigation: documented; community fixups can provide `replace_deps` to system packages.
- **Native sdist deferral.** v1 errors instead of handling. Acceptable for launch; the credible launch surface (numpy/pandas/scipy/torch/cryptography/requests/fastapi/ruff) all ship wheels. Real concern: long-tail packages without wheels will need fixups or v2.
- **Determinism vs uv.lock churn.** uv may rewrite `uv.lock` with cosmetic changes. Mitigation: muntjac's input is the resolved data, not the file bytes — same resolved graph ⇒ same BUCK.
- **Multi-tree composability.** Two trees sharing the same package at different versions: each gets its own targets. No risk of accidental version clash because target paths differ (`//third-party/python/modern:tensorflow` vs `//third-party/python/legacy:tensorflow`). Documented as the explicit semantics.
- **Registry repo location undecided.** Spec uses `github.com/<user>/muntjac-fixups` as a placeholder. Real location is a launch-time decision: under the muntjac author's GitHub, under a neutral org, or hosted in a new org. Doesn't block the v0.1.0 implementation — only the `registry_rev` pin needs to be writable to the chosen URL before the first public release.

---

## 12. Future work (v2+)

Captured, not committed:

- **Native sdist Buck-time builds.** PEP 517 in Buck. Schema hooks already in place (§7 `[sdist]`).
- **Windows support.** `win_amd64` wheel tags + Buck2-on-Windows toolchain.
- **Bazel output backend.** Second emitter under `src/bazel/`.
- **Free-threaded Python (`cp3Xt`).** Selector + config expansion.
- **`muntjac diff` / impact analysis.** Compare two lockfiles, show which Buck targets churn.
- **LSP / editor integration.** Use `src/lib.rs` as a library; surface fixup-aware completions in `fixups.toml` editing.
