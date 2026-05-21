# Muntjac S4 — Multi-platform + multi-python BUCK — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Promote the S3 single-platform BUCK emitter to a multi-platform (3 platforms × 2 pythons) emitter that produces working `buck2 build`+`run` against a real numpy demo on three CI runners.

**Architecture:** Extends `src/buck/emit.rs` with an `EmitDeps` enum (Uniform | PerCell) so cell-varying deps render as `deps = select({...})` instead of erroring. Generated `PACKAGE` gains `set_cfg_modifiers` wiring for host OS+CPU; python version is documented as a user-side `modifiers = [...]` attr. CI installs `buck2`, regenerates the `02-numpy-pandas` fixture, and runs an in-fixture `python_binary` that imports numpy. Folds in three tech-debt items: `LockfileError::BadUrl`, iterative Tarjan SCC, and a design-spec edit lifting the free-threaded Python non-goal.

**Tech Stack:** Rust 2024 (existing), `anyhow` for handler errors, `insta` for snapshot tests, `tempfile` + `assert_cmd` for integration tests. Buck2 (binary install) for CI smoke. GitHub Actions matrix on `ubuntu-latest`, `ubuntu-24.04-arm`, `macos-latest`.

---

## File structure

**New files:**

| Path | Responsibility |
|---|---|
| `tests/fixtures/buck/02-numpy-pandas/muntjac.toml` | 3-platform × 2-python config, `third_party_dir = "third-party/python"`. |
| `tests/fixtures/buck/02-numpy-pandas/pyproject.toml` | Workspace root with single dep `numpy>=2.1,<2.3`. |
| `tests/fixtures/buck/02-numpy-pandas/uv.lock` | Frozen lockfile (committed). |
| `tests/fixtures/buck/02-numpy-pandas/.buckconfig` | Buck2 cell config — prelude pointer + cell aliases. Exact contents from spike. |
| `tests/fixtures/buck/02-numpy-pandas/tests/smoke/BUCK` | Hand-written `python_binary` with `modifiers = ["//third-party/python/config:py312"]`. |
| `tests/fixtures/buck/02-numpy-pandas/tests/smoke/demo.py` | `import numpy as np; arr=np.zeros(3); print(arr); assert arr.shape==(3,)`. |
| `tests/fixtures/buck/02-numpy-pandas/expected/BUCK` | Golden output (one `pypi_package` per resolved package). |
| `tests/fixtures/buck/02-numpy-pandas/expected/muntjac.bzl` | Golden with 6-cell `_CONFIGS` + python-axis wiring header. |
| `tests/fixtures/buck/02-numpy-pandas/expected/PACKAGE` | Golden with host-axis `set_cfg_modifiers`. |
| `tests/fixtures/buck/02-numpy-pandas/expected/config/BUCK` | Golden with 5 constraint_values + 6 config_settings. |
| `tests/fixtures/buck/03-musllinux/muntjac.toml` | 1-platform (`linux-x86_64-musl`) × 1-python (3.12) config. |
| `tests/fixtures/buck/03-musllinux/pyproject.toml` | Workspace root with one dep that publishes musllinux wheels. |
| `tests/fixtures/buck/03-musllinux/uv.lock` | Frozen. |
| `tests/fixtures/buck/03-musllinux/expected/*` | Golden output asserting `*musllinux*` in wheel URL. |

**Modified files:**

| Path | Change |
|---|---|
| `src/buck/emit.rs` | Add `EmitDeps` enum; change `EmitPackage::deps` type; replace `bail!()` at L184 with collapse-uniform logic; update unit tests. |
| `src/buck/string_writer.rs` | Render `deps = select({...})` for `PerCell`; render `deps = [...]` for `Uniform`; extend muntjac.bzl with python-axis wiring header; render real PACKAGE with `set_cfg_modifiers`; extend config/BUCK for multi-platform. |
| `src/error.rs` | New `LockfileError::BadUrl` variant. |
| `src/lock/parser.rs` | Migrate sdist/wheel/git URL parse failures from `BadVersion` → `BadUrl`. Update tests. |
| `src/lock/graph.rs` | Convert `strongconnect` from recursive to iterative. |
| `tests/fixtures/buck/01-pure-python/expected/PACKAGE` | Regenerate for new auto-wiring shape. |
| `tests/fixtures/buck/01-pure-python/expected/config/BUCK` | Regenerate if host-axis constraint_values are added. |
| `tests/buckify.rs` | Add `fixture_02_numpy_pandas_golden` + `fixture_03_musllinux_golden` integration tests. |
| `tests/fixtures/buck/README.md` | Append paragraph on multi-cell fixture convention. |
| `.github/workflows/ci.yml` | Add buck2 install step + muntjac buckify + buck2 run smoke step on all three matrix runners. |
| `docs/superpowers/specs/2026-05-20-muntjac-design.md` | Remove free-threaded Python non-goal in §1. |
| `docs/superpowers/TECH_DEBT.md` | Move `BadVersion→BadUrl`, `Tarjan iterative` to Resolved; retarget `cp313t` from "S4+" to "S5 or later". |
| `docs/superpowers/specs/2026-05-20-muntjac-roadmap.md` | Mark S4 ✅ shipped at end. |

---

## Phase ordering rationale

Per spec §10, the cfg-modifier API surface needs validating against real buck2 before goldens are frozen. Order:

1. **Phase 0** — fixture inputs (no emitter changes; buckify against current S3 emitter still works on these). Required as scaffolding for the spike.
2. **Phase 1** — spike: hand-write `BUCK` + `PACKAGE` + `config/BUCK` until `buck2 run //tests/smoke:numpy_demo` is green. Output: validated snippets + buck2 install procedure.
3. **Phase 2** — emitter changes for per-cell deps machinery (independent of spike; can run parallel with Phase 1 if subagent-driven, but the plan lays them sequentially for clarity).
4. **Phase 3** — emitter rendering (matches validated spike output).
5. **Phase 4** — goldens + integration tests.
6. **Phase 5** — CI workflow.
7. **Phase 6** — tech-debt fold-ins (parallel-eligible at any point; placed here to keep the critical path tight).
8. **Phase 7** — docs.

---

## Phase 0 — Fixture inputs (scaffolding for the spike)

### Task 1: Create `02-numpy-pandas` fixture inputs

**Files:**
- Create: `tests/fixtures/buck/02-numpy-pandas/muntjac.toml`
- Create: `tests/fixtures/buck/02-numpy-pandas/pyproject.toml`
- Create: `tests/fixtures/buck/02-numpy-pandas/uv.lock`
- Create: `tests/fixtures/buck/02-numpy-pandas/.gitignore`

The fixture root will eventually hold a `.buckconfig` (Task 6), a generated `third-party/python/` directory (Tasks 8+; gitignored), `tests/smoke/` (Task 7), and `expected/` (Task 18). This task lays down only the inputs.

- [ ] **Step 1: Create the fixture directory**

```bash
mkdir -p tests/fixtures/buck/02-numpy-pandas
cd tests/fixtures/buck/02-numpy-pandas
```

- [ ] **Step 2: Write `muntjac.toml`**

```toml
manifest_path   = "pyproject.toml"
third_party_dir = "third-party/python"
python_versions = ["3.11", "3.12"]

[platforms]
linux-x86_64-gnu  = { target = "x86_64-unknown-linux-gnu",  manylinux = "2_17" }
linux-aarch64-gnu = { target = "aarch64-unknown-linux-gnu", manylinux = "2_17" }
macos-arm64       = { target = "aarch64-apple-darwin",      macos_min = "11.0" }

[fixups]
registry              = "none"
allow_local_overrides = false

[buck]
file_name = "BUCK"
vendor    = false
```

- [ ] **Step 3: Write `pyproject.toml`**

```toml
[project]
name = "muntjac-fixture-02-numpy-pandas"
version = "0.0.0"
requires-python = ">=3.11,<3.13"
dependencies = ["numpy>=2.1,<2.3"]

[build-system]
requires = ["hatchling"]
build-backend = "hatchling.build"
```

- [ ] **Step 4: Generate `uv.lock`**

You need a system `uv` (the resolver) for this. The version doesn't matter much, but pin in the commit message which one you used.

```bash
uv lock --refresh
ls -la uv.lock
```

Expected: `uv.lock` exists in the fixture root. Inspect briefly — it should contain numpy and no other runtime deps (numpy 2.x has none). The file will be ~70-150 KB; that's fine.

- [ ] **Step 5: Write `.gitignore`**

The generated `third-party/python/` directory and any `buck-out/` from local buck2 runs should not be committed:

```
# muntjac-generated; reproduced by `cargo run -- buckify`
third-party/

# buck2 build outputs (local + CI)
buck-out/
.buckd/

# uv's venv if developer creates one
.venv/
```

- [ ] **Step 6: Verify the inputs are well-formed**

Run muntjac's existing config-check + print-deps to confirm the inputs parse:

```bash
cd /home/jackm/repos/muntjac
cargo run --release -- -C tests/fixtures/buck/02-numpy-pandas check-config 2>&1 | tail -5
```

Expected: exit 0, no error output. The S3 buckify command WILL bail here (because we haven't extended the emitter for multi-platform), so don't run it yet.

```bash
cargo run --release -- -C tests/fixtures/buck/02-numpy-pandas debug print-deps 2>&1 | head -30
```

Expected: prints resolved deps as JSON; should include numpy entries for each `(platform, python_version)` cell.

- [ ] **Step 7: Commit**

```bash
git add tests/fixtures/buck/02-numpy-pandas/
git commit -m "test(s4): scaffold 02-numpy-pandas fixture inputs

3 platforms (linux-x86_64-gnu, linux-aarch64-gnu, macos-arm64) x
2 pythons (3.11, 3.12) = 6 cells. Single dep: numpy>=2.1,<2.3.
uv.lock frozen with uv <version>. Scaffolds inputs; expected/ and
tests/smoke/ land in later tasks once the spike validates the
buck2 wiring shape."
```

---

### Task 2: Create `03-musllinux` fixture inputs

**Files:**
- Create: `tests/fixtures/buck/03-musllinux/muntjac.toml`
- Create: `tests/fixtures/buck/03-musllinux/pyproject.toml`
- Create: `tests/fixtures/buck/03-musllinux/uv.lock`
- Create: `tests/fixtures/buck/03-musllinux/.gitignore`

The musllinux fixture is emit-only — no `tests/smoke/`, no `.buckconfig`, no CI build step. It only exercises the selector.

Initial package choice: **`psycopg2-binary`**. It publishes both manylinux and musllinux wheels, and on `linux-x86_64-musl` only the musllinux variant matches. If this becomes impractical (uv.lock failures, transitive sdists muntjac can't handle in S4 since native-sdist support is S5), substitute another package — `cffi`, `lxml`, or `cryptography` are candidates. The fixture's value is the *selector* assertion, not the specific package.

- [ ] **Step 1: Create the directory + scaffold**

```bash
mkdir -p tests/fixtures/buck/03-musllinux
cd tests/fixtures/buck/03-musllinux
```

- [ ] **Step 2: Write `muntjac.toml`**

```toml
manifest_path   = "pyproject.toml"
third_party_dir = "third-party/python"
python_versions = ["3.12"]

[platforms]
linux-x86_64-musl = { target = "x86_64-unknown-linux-musl", musllinux = "1_2" }

[fixups]
registry              = "none"
allow_local_overrides = false

[buck]
file_name = "BUCK"
vendor    = false
```

- [ ] **Step 3: Write `pyproject.toml` (try psycopg2-binary first)**

```toml
[project]
name = "muntjac-fixture-03-musllinux"
version = "0.0.0"
requires-python = ">=3.12,<3.13"
dependencies = ["psycopg2-binary>=2.9,<3.0"]

[build-system]
requires = ["hatchling"]
build-backend = "hatchling.build"
```

- [ ] **Step 4: Generate `uv.lock`**

```bash
uv lock --refresh
```

If `uv lock` produces a lockfile where `psycopg2-binary` has only sdist (not wheels) for `linux-x86_64-musl`, S4's emitter will error with the S3 `NoWheel` message. In that case:

a. Run `muntjac debug pick-wheels` (or inspect the lockfile manually) to confirm which wheel was picked.
b. If the picked wheel filename contains `musllinux`, you're good — proceed.
c. If not (e.g. all musllinux wheels are absent), substitute another package: try `cffi`, then `lxml`, then `cryptography`. Each is similarly liable to publish musllinux variants. The fixture's `pyproject.toml` is the only file that changes.

Document the picked package in the commit message.

- [ ] **Step 5: Verify the selector picks a musllinux wheel**

```bash
cd /home/jackm/repos/muntjac
cargo run --release -- -C tests/fixtures/buck/03-musllinux debug pick-wheels 2>&1 | tail -20
```

Expected: the output table shows `psycopg2-binary` (or whichever package you settled on) at `(py312, linux-x86_64-musl)` mapped to a wheel filename containing `musllinux_1_2_x86_64`.

If you see `NO_WHEEL`, pick a different package and try again.

- [ ] **Step 6: Write `.gitignore`**

```
third-party/
buck-out/
.buckd/
.venv/
```

- [ ] **Step 7: Commit**

```bash
git add tests/fixtures/buck/03-musllinux/
git commit -m "test(s4): scaffold 03-musllinux fixture inputs

1 platform (linux-x86_64-musl) x 1 python (3.12). Single dep:
<package> chosen because it ships musllinux_1_2_x86_64 wheels.
Frozen uv.lock. expected/ lands once the multi-cell emitter is in
place."
```

---

## Phase 1 — Spike: validate buck2 + cfg-modifier API

This phase is exploratory. The exact `.buckconfig`, prelude bootstrap procedure, and cfg-modifier `ModifiersMatch` shape are unknown until validated against real `buck2`. Steps are exploration with success criteria rather than tight TDD.

**Output of Phase 1:** a working hand-written `02-numpy-pandas/tests/smoke/` + `02-numpy-pandas/.buckconfig` + hand-crafted `02-numpy-pandas/third-party/python/{BUCK,muntjac.bzl,PACKAGE,config/BUCK}` such that `buck2 run //tests/smoke:numpy_demo` succeeds. Plus a buck2 install procedure that CI can copy.

### Task 3: Install buck2 locally; verify

**Files:** none modified (this is an environment task)

- [ ] **Step 1: Pick a buck2 version to pin**

Visit `https://github.com/facebook/buck2/releases` (or `gh release list --repo facebook/buck2 --limit 10`) and pick the most recent stable release (look for `latest` tag, not pre-release).

Record:
- Release tag (e.g. `2026-04-15`).
- Asset name for your host triple (e.g. `buck2-x86_64-unknown-linux-gnu.zst`).

Document this in a scratch note (`docs/superpowers/scratch-s4-spike.md` if you want, or just in your shell history). The version becomes the CI pin in Task 25.

- [ ] **Step 2: Install buck2 to `~/.local/bin/buck2`**

```bash
RELEASE_TAG="<chosen-tag>"
ASSET="buck2-x86_64-unknown-linux-gnu.zst"  # adapt for your host
mkdir -p ~/.local/bin
curl -L "https://github.com/facebook/buck2/releases/download/${RELEASE_TAG}/${ASSET}" \
    -o /tmp/buck2.zst
zstd -d /tmp/buck2.zst -o ~/.local/bin/buck2
chmod +x ~/.local/bin/buck2
buck2 --version
```

Expected: `buck2 <date> <git-sha>` printed.

If `zstd` is not installed: `pacman -S zstd` (or `apt-get install zstd`).

- [ ] **Step 3: Verify buck2 runs**

```bash
buck2 --help | head -20
```

Expected: usage banner with subcommands `build`, `run`, `test`, etc.

This task does not need a commit (no files in the repo changed).

---

### Task 4: Bootstrap a buck2 cell + prelude in the fixture

**Files:**
- Create: `tests/fixtures/buck/02-numpy-pandas/.buckconfig`
- Possibly modify: `tests/fixtures/buck/02-numpy-pandas/.gitignore`
- Possibly add: a git submodule reference, OR an extracted prelude tarball, OR a CI-only fetch script

The buck2 prelude (`@prelude//`) needs to be reachable from the fixture's cell. Open-source buck2 setups typically use one of three approaches; pick whichever works.

- [ ] **Step 1: Read buck2 docs on cell setup**

```bash
buck2 init --help 2>&1 | head -40
```

If `buck2 init` exists and offers a "scaffold a project" mode, use it. Otherwise consult `https://buck2.build/docs/about/getting_started/` for the manual setup.

The typical minimum is a `.buckconfig` declaring `[cells]` (which directories are which cells) and `[parser]` (`target_platform_detector_spec` if needed).

- [ ] **Step 2: Choose a prelude-fetch strategy**

Options:

**a. Git submodule.** Add `facebook/buck2-prelude` as a submodule at `tests/fixtures/buck/02-numpy-pandas/prelude/`. Pros: declarative, version-pinned via submodule SHA. Cons: submodule overhead, CI checkout-submodules step.

**b. Vendored copy.** Check out the prelude once, copy into the fixture, commit the whole thing. Pros: zero CI setup. Cons: huge diff in the muntjac repo (~thousands of files), update friction.

**c. Fetched-in-CI.** A `prelude-fetch.sh` script that clones `facebook/buck2-prelude` at a pinned SHA into `prelude/`. Run before `buck2 run`. Pros: small repo footprint, easy update. Cons: needs network access; CI run-time cost (~5-10s).

Recommended: **(a) git submodule**. It's the standard buck2-open-source pattern and what most third-party setups use.

- [ ] **Step 3: Add the prelude submodule**

```bash
cd tests/fixtures/buck/02-numpy-pandas
git submodule add https://github.com/facebook/buck2-prelude.git prelude
cd prelude
git checkout <pinned-prelude-sha>   # use a SHA matching the buck2 binary version
cd ..
git add prelude .gitmodules
```

The pinned SHA: pick one that matches your buck2 binary version. From `buck2 --version` you get the buck2 binary's date; pick a prelude commit from around the same date. The prelude release notes (if any) on `facebook/buck2-prelude` may help. Worst case: pick `main` at the day of the buck2 binary release.

- [ ] **Step 4: Write `.buckconfig`**

```ini
[cells]
root   = .
prelude = prelude

[cell_aliases]
config  = prelude
fbcode  = root
fbsource = root

[parser]
target_platform_detector_spec = target:root//...->prelude//platforms:default
```

Adjust based on what `buck2 init` (Step 1) revealed if it offered a template. The above is a starting guess — the spike's job is to iterate this until things work.

- [ ] **Step 5: Verify the cell is recognized**

```bash
cd tests/fixtures/buck/02-numpy-pandas
buck2 audit cells 2>&1 | head -10
```

Expected: `buck2` recognizes `root`, `prelude`, plus the aliases. If you see "ERROR: ...", read the message and tune `.buckconfig`. Common issues: missing `target_platform_detector_spec`, wrong relative path to prelude.

- [ ] **Step 6: Update `.gitignore` if needed**

If buck2 created a `buck-out/`, `.buckd/`, or `.buck2-cache/` directory in the fixture, confirm these are in `.gitignore` from Task 1 — they should be. Add any others you see.

- [ ] **Step 7: Commit the cell scaffold**

```bash
git add tests/fixtures/buck/02-numpy-pandas/.buckconfig \
        tests/fixtures/buck/02-numpy-pandas/.gitmodules \
        .gitmodules
git commit -m "test(s4): bootstrap buck2 cell + prelude submodule for 02-numpy-pandas

Buck2 release <tag>; prelude pinned to <sha>. .buckconfig declares
root + prelude cells with config// alias for prelude (standard
open-source convention)."
```

---

### Task 5: Hand-write the smoke target + `BUCK` + `muntjac.bzl` + `PACKAGE` + `config/BUCK` until `buck2 run` is green

**Files (all hand-written, eventually replaced/regenerated by the emitter):**
- Create: `tests/fixtures/buck/02-numpy-pandas/tests/smoke/BUCK`
- Create: `tests/fixtures/buck/02-numpy-pandas/tests/smoke/demo.py`
- Create: `tests/fixtures/buck/02-numpy-pandas/third-party/python/BUCK`
- Create: `tests/fixtures/buck/02-numpy-pandas/third-party/python/muntjac.bzl`
- Create: `tests/fixtures/buck/02-numpy-pandas/third-party/python/PACKAGE`
- Create: `tests/fixtures/buck/02-numpy-pandas/third-party/python/config/BUCK`

This is the spike's main loop. You'll write all these files by hand, run `buck2 run`, fix what breaks, repeat. The files will eventually be replaced — the `third-party/python/*` files are `.gitignored` (per Task 1) and the emitter will regenerate them. `tests/smoke/*` will stay hand-written and committed (Task 7).

- [ ] **Step 1: Write `tests/smoke/demo.py`**

```python
import numpy as np
arr = np.zeros(3)
print(arr)
assert arr.shape == (3,), f"unexpected shape {arr.shape}"
```

- [ ] **Step 2: Write `tests/smoke/BUCK`**

```python
load("@prelude//python:python_binary.bzl", "python_binary")

python_binary(
    name = "numpy_demo",
    main = "demo.py",
    modifiers = ["//third-party/python/config:py312"],
    deps = ["//third-party/python:numpy"],
)
```

Note: confirm `python_binary` is loaded from the right path. If `buck2` errors with "no such module," check `prelude/python/` directory listing and adjust.

- [ ] **Step 3: Write a minimal `third-party/python/config/BUCK`**

Based on spec §5, but write it by hand. The cell prefix for OS/CPU constraints is the unknown to verify:

```python
constraint_setting(name = "python_version")
constraint_value(name = "py311", constraint_setting = ":python_version")
constraint_value(name = "py312", constraint_setting = ":python_version")

constraint_setting(name = "platform")
constraint_value(name = "linux-x86_64-gnu",  constraint_setting = ":platform")
constraint_value(name = "linux-aarch64-gnu", constraint_setting = ":platform")
constraint_value(name = "macos-arm64",       constraint_setting = ":platform")

config_setting(
    name = "py312-linux-x86_64-gnu",
    constraint_values = [":linux-x86_64-gnu", ":py312"],
)
# ... five more, for all 6 cells. Write all six.
```

- [ ] **Step 4: Write a minimal `third-party/python/PACKAGE` (try the spec §5 shape first)**

```python
load("@prelude//cfg/modifier:set_cfg_modifiers.bzl", "set_cfg_modifiers")

set_cfg_modifiers(
    cfg_modifiers = [
        {
            "config//os/constraints:linux": {
                "config//cpu/constraints:x86_64": "//third-party/python/config:linux-x86_64-gnu",
                "config//cpu/constraints:arm64":  "//third-party/python/config:linux-aarch64-gnu",
            },
            "config//os/constraints:macos": {
                "config//cpu/constraints:arm64":  "//third-party/python/config:macos-arm64",
            },
        },
    ],
)
```

When `buck2 run` fails with "no such target `config//os/constraints:linux`", switch to whatever cell prefix the open-source prelude actually uses. Inspect `prelude/` for files like `prelude/os/constraints/BUCK` or `prelude/platforms/...`. Adjust the keys.

- [ ] **Step 5: Write a minimal `third-party/python/muntjac.bzl`**

Lift the body from spec §5 — the wiring header comment is optional during the spike (add it once everything works); focus first on the macro body:

```python
load("@prelude//python:prebuilt_python_library.bzl", "prebuilt_python_library")
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
    expect(set(wheels.keys()).issubset(set(_CONFIGS)), "unknown config in {}".format(name))

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

Confirm the `load()` paths. `http_file` may be a built-in or need a load; check by running `buck2 run` and reading errors.

- [ ] **Step 6: Write a minimal `third-party/python/BUCK`**

Just one `pypi_package` call — numpy — with all 6 cells. Lift the URLs from `uv.lock` (each entry has `url = "..."` and `hash = "sha256:..."`):

```python
load("//third-party/python:muntjac.bzl", "pypi_package")

pypi_package(
    name = "numpy",
    version = "2.1.<patch>",  # match your uv.lock
    deps = [],
    wheels = {
        "py311-linux-aarch64-gnu": ("https://files.pythonhosted.org/.../numpy-2.1.X-cp311-cp311-manylinux_2_17_aarch64.manylinux2014_aarch64.whl", "sha256:..."),
        "py311-linux-x86_64-gnu":  ("https://files.pythonhosted.org/.../numpy-2.1.X-cp311-cp311-manylinux_2_17_x86_64.manylinux2014_x86_64.whl",   "sha256:..."),
        "py311-macos-arm64":       ("https://files.pythonhosted.org/.../numpy-2.1.X-cp311-cp311-macosx_14_0_arm64.whl",                            "sha256:..."),
        "py312-linux-aarch64-gnu": ("https://files.pythonhosted.org/.../numpy-2.1.X-cp312-cp312-manylinux_2_17_aarch64.manylinux2014_aarch64.whl", "sha256:..."),
        "py312-linux-x86_64-gnu":  ("https://files.pythonhosted.org/.../numpy-2.1.X-cp312-cp312-manylinux_2_17_x86_64.manylinux2014_x86_64.whl",   "sha256:..."),
        "py312-macos-arm64":       ("https://files.pythonhosted.org/.../numpy-2.1.X-cp312-cp312-macosx_14_0_arm64.whl",                            "sha256:..."),
    },
    visibility = ["PUBLIC"],
)
```

Fill the actual URL + sha256 from `uv.lock`. The `<patch>` and `_<X>` numpy variant suffix will reflect what `uv lock` actually picked.

- [ ] **Step 7: Run `buck2 run` — first iteration**

```bash
cd tests/fixtures/buck/02-numpy-pandas
buck2 run //tests/smoke:numpy_demo 2>&1 | tail -40
```

Almost certainly fails on the first try. Common issues + fixes:

- **"no such target `config//...`"**: cell prefix is wrong. Try `prelude//constraints/os:linux` instead, or whatever shows up in `find prelude -name 'BUCK' | xargs grep 'constraint_value'`.
- **"`http_file` is not defined"**: needs a `load("@prelude//http/...")`. Inspect prelude for the correct path.
- **"`set_cfg_modifiers` is only allowed in PACKAGE files"**: make sure you're saving to `PACKAGE`, not `BUCK`.
- **"select() resolved no key"**: the modifier didn't fire — either the constraint values aren't being set, or the `config_setting` doesn't match the active configuration. Run `buck2 cquery //third-party/python:numpy 2>&1 | tail -10` to see what configuration buck2 picked.
- **"sha256 mismatch"**: the wheel URL doesn't match the sha you copied — confirm both came from the same `uv.lock` entry.
- **"prebuilt_python_library not found"**: try `load("@prelude//python:python_library.bzl", "prebuilt_python_library")` instead of the path in my Step 5 draft.
- **Python interpreter not found**: buck2 needs a python toolchain. Check `prelude/toolchains/python.bzl`. May need to register a `system_python_toolchain` in the fixture's BUCK or pass `--config python.interpreter=$(which python3)`.

Iterate. **Do not give up on shape; only adjust paths/load() to match reality.** The shape (PACKAGE auto-wiring host axis; alias-with-select picking per cell; user-side modifier for python version) must remain — it's what the design committed to.

- [ ] **Step 8: Document what worked**

Once `buck2 run //tests/smoke:numpy_demo` prints `[0. 0. 0.]` and exits 0, copy the working snippets into a new file:

```bash
# In the muntjac repo root, NOT the fixture dir:
cat > docs/superpowers/scratch-s4-spike.md <<'EOF'
# S4 spike findings (delete this file at end of S4)

## buck2 install
- Version: <pinned-tag>
- URL: https://github.com/facebook/buck2/releases/download/<tag>/<asset>
- Install: curl + zstd

## prelude
- Submodule URL: https://github.com/facebook/buck2-prelude.git
- Pinned SHA: <sha>

## .buckconfig that worked
```ini
<paste your final working .buckconfig>
```

## Validated cell prefix for OS/CPU
- OS constraint setting: `<actual-prefix>//os/constraints:linux` (etc)
- CPU constraint setting: `<actual-prefix>//cpu/constraints:x86_64` (etc)

## Validated set_cfg_modifiers shape
```python
<paste your final working set_cfg_modifiers call>
```

## Validated load() paths
- python_binary: <load path>
- prebuilt_python_library: <load path>
- http_file: <load path or built-in?>
- set_cfg_modifiers: <load path>
EOF
```

These notes drive Phase 3 emitter rendering.

- [ ] **Step 9: Sanity-check: clean and re-run**

```bash
cd tests/fixtures/buck/02-numpy-pandas
buck2 clean 2>&1 | tail -3
buck2 run //tests/smoke:numpy_demo 2>&1 | tail -5
```

Expected: `[0. 0. 0.]` printed; exit 0. Confirms the build is reproducible from a clean state.

- [ ] **Step 10: Commit `tests/smoke/` only**

`third-party/python/*` is gitignored — do NOT commit those hand-written files. They'll be regenerated by the emitter in Phase 4. Commit only `tests/smoke/` and the spike notes:

```bash
git add tests/fixtures/buck/02-numpy-pandas/tests/smoke/ \
        docs/superpowers/scratch-s4-spike.md
git commit -m "test(s4): commit tests/smoke for 02-numpy-pandas; record spike findings

Spike validated buck2 <tag> + prelude SHA <sha> + open-source-prelude
cfg-modifier surface. tests/smoke/{BUCK,demo.py} is the hand-written
'wiring works' worked example consumed by CI smoke. Hand-written
third-party/python/ files used during the spike are gitignored; the
emitter regenerates them in Phase 3-4.

scratch-s4-spike.md captures validated load() paths, cell prefixes,
and the set_cfg_modifiers shape — drives Phase 3 emitter rendering."
```

---

## Phase 2 — Emitter: per-cell deps machinery

The emitter pipeline already builds `pkg_deps_per_cell: BTreeMap<PkgKey, BTreeMap<ConfigName, Vec<String>>>` (S3 code, `src/buck/emit.rs:122`). Phase 2 generalizes `EmitPackage::deps` to express either uniform or per-cell, then replaces the existing `bail!()` on mismatch with the collapse-uniform logic.

### Task 6: Add `EmitDeps` enum to `src/buck/emit.rs`

**Files:**
- Modify: `src/buck/emit.rs`

- [ ] **Step 1: Read the current `EmitPackage`**

```bash
sed -n '18,30p' src/buck/emit.rs
```

Confirm the current shape: `pub deps: Vec<String>`, `pub wheels: BTreeMap<ConfigName, EmitWheel>`.

- [ ] **Step 2: Add the failing test for `EmitDeps`**

In `src/buck/emit.rs::tests`, add a new test:

```rust
#[test]
fn emit_deps_variants_construct() {
    let uniform = EmitDeps::Uniform(vec![":foo".into(), ":bar".into()]);
    let per_cell = EmitDeps::PerCell({
        let mut m = BTreeMap::new();
        m.insert(
            ConfigName::new("3.12", "linux-x86_64-gnu"),
            vec![":foo".into()],
        );
        m.insert(
            ConfigName::new("3.11", "linux-x86_64-gnu"),
            vec![":foo".into(), ":bar".into()],
        );
        m
    });

    match uniform {
        EmitDeps::Uniform(v) => assert_eq!(v.len(), 2),
        EmitDeps::PerCell(_) => panic!("expected Uniform"),
    }
    match per_cell {
        EmitDeps::PerCell(m) => assert_eq!(m.len(), 2),
        EmitDeps::Uniform(_) => panic!("expected PerCell"),
    }
}
```

- [ ] **Step 3: Run — verify fails**

```bash
cargo test --lib buck::emit::tests::emit_deps_variants_construct 2>&1 | tail -10
```

Expected: FAIL — `EmitDeps` not defined.

- [ ] **Step 4: Define `EmitDeps`**

Add to `src/buck/emit.rs` (above the existing `EmitPackage` declaration):

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EmitDeps {
    /// Every cell has the same dep list. Rendered as `deps = [":foo", ":bar"]`.
    Uniform(Vec<String>),

    /// Cells differ. Rendered as `deps = select({...})` with one branch per cell.
    /// Keys are every cell in `EmitInput::configs` (no `default` arm — cell
    /// coverage is exhaustive by construction in `build_emit_input`).
    PerCell(BTreeMap<ConfigName, Vec<String>>),
}
```

- [ ] **Step 5: Run — verify passes**

```bash
cargo test --lib buck::emit::tests::emit_deps_variants_construct 2>&1 | tail -5
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src/buck/emit.rs
git commit -m "feat(s4): add EmitDeps enum (Uniform | PerCell) in buck::emit

Generalizes per-cell dep representation ahead of replacing the S3
dep-set cross-cell-mismatch bail!() with select() rendering. The
field on EmitPackage flips in the next task."
```

---

### Task 7: Change `EmitPackage::deps` field type to `EmitDeps`

**Files:**
- Modify: `src/buck/emit.rs`

This step cascades — anything that constructed an `EmitPackage` with `deps: Vec<String>` won't compile. We fix the callers as we go.

- [ ] **Step 1: Change the field type**

In `src/buck/emit.rs`:

```rust
pub struct EmitPackage {
    pub name: String,
    pub version: String,
    pub deps: EmitDeps,           // was: Vec<String>
    pub wheels: BTreeMap<ConfigName, EmitWheel>,
}
```

- [ ] **Step 2: Run — capture compile errors**

```bash
cargo build 2>&1 | grep -E "error\[" | head -20
```

Expected: errors at call sites that constructed `EmitPackage`. Likely:
- `src/buck/emit.rs::tests::emit_input_constructs` — has `deps: vec![":certifi".into(), ":idna".into()]`.
- `src/buck/emit.rs::tests::build_emit_input_from_synthetic_resolved` — possibly checks `pkg.deps.is_empty()`.
- `src/buck/emit.rs::build_emit_input` — constructs `EmitPackage { deps, ... }` at line ~207.
- `src/buck/string_writer.rs` — reads `pkg.deps` for rendering.

- [ ] **Step 3: Fix `emit_input_constructs` test**

In `src/buck/emit.rs::tests::emit_input_constructs`:

```rust
deps: EmitDeps::Uniform(vec![":certifi".into(), ":idna".into()]),
```

- [ ] **Step 4: Fix `build_emit_input_from_synthetic_resolved` test**

Find any `assert!(pkg.deps.is_empty())` or `assert_eq!(pkg.deps, ...)` and replace:

```rust
match &pkg.deps {
    EmitDeps::Uniform(v) => assert!(v.is_empty()),
    EmitDeps::PerCell(_) => panic!("expected Uniform for empty-deps case"),
}
```

- [ ] **Step 5: Stub the `build_emit_input` caller**

In `build_emit_input`, the `packages.push(EmitPackage { ... deps, ... })` call needs to wrap the existing `deps: Vec<String>` in `EmitDeps::Uniform(deps)`. We'll replace this with the collapse-uniform logic in the next task. For now:

```rust
packages.push(EmitPackage {
    name: key.0,
    version: key.1,
    deps: EmitDeps::Uniform(deps),  // temporary — Task 8 replaces this
    wheels: wheel_map,
});
```

- [ ] **Step 6: Stub `string_writer.rs` to compile**

In `src/buck/string_writer.rs`, find where `pkg.deps` is iterated (probably formatting `[":foo", ":bar"]`). Wrap in a `match` so it compiles for now; Task 11/12 fills in the proper rendering:

```rust
let deps_iter: Vec<&String> = match &pkg.deps {
    EmitDeps::Uniform(v) => v.iter().collect(),
    EmitDeps::PerCell(_) => {
        // Task 12 replaces this branch with select() rendering.
        Vec::new()
    }
};
```

This is a deliberate placeholder that the next two tasks remove. The string_writer's existing snapshot tests will still pass for the Uniform case.

- [ ] **Step 7: Verify build compiles + tests pass**

```bash
cargo build 2>&1 | tail -5
cargo test 2>&1 | tail -5
```

Expected: all 145 S3-baseline tests pass + the 1 new `emit_deps_variants_construct` = 146 tests.

- [ ] **Step 8: Commit**

```bash
git add src/buck/emit.rs src/buck/string_writer.rs
git commit -m "refactor(s4): flip EmitPackage::deps to EmitDeps enum

Cascade through emit.rs tests + build_emit_input + string_writer. The
build_emit_input call site temporarily wraps deps in Uniform() — Task
8 replaces this with collapse-uniform logic. string_writer's PerCell
branch is a placeholder until Task 12."
```

---

### Task 8: Replace `bail!()` with collapse-uniform logic in `build_emit_input`

**Files:**
- Modify: `src/buck/emit.rs`

The S3 code at `src/buck/emit.rs:178-194` bails on cross-cell dep mismatch. Replace it with: build the full per-cell map, then collapse to `Uniform` if all values are equal, else keep `PerCell`.

- [ ] **Step 1: Read the current logic**

```bash
sed -n '174,225p' src/buck/emit.rs
```

You should see the cross-cell-mismatch bail, the dep formatting (split on `@`, prepend `:`, sort+dedup), and the `packages.push(...)` call.

- [ ] **Step 2: Write a failing test for `PerCell` preservation**

In `src/buck/emit.rs::tests`, add a new test using a synthetic lockfile with marker-gated deps. Use the existing `build_emit_input_from_synthetic_resolved` test as a template. The new test should construct a package whose deps differ between two cells:

```rust
#[test]
fn build_emit_input_produces_per_cell_when_deps_differ() {
    use crate::config::{Config, Platform, PythonVersion, Tree};
    use crate::lock::types::{DepEdge, FirstPartyKind, Lockfile, Package, Source, Wheel};
    use pep440_rs::Version;
    use pep508_rs::PackageName;
    use pep508_rs::MarkerTree;
    use std::str::FromStr;
    use url::Url;

    let tree = Tree {
        name: "default".into(),
        manifest_path: "pyproject.toml".into(),
        third_party_dir: "third-party/python".into(),
        python_versions: vec![PythonVersion(3, 11), PythonVersion(3, 12)],
    };

    let mut platforms = std::collections::BTreeMap::new();
    platforms.insert(
        "linux-x86_64-gnu".into(),
        Platform {
            target: "x86_64-unknown-linux-gnu".into(),
            manylinux: Some("2_17".into()),
            musllinux: None,
            macos_min: None,
        },
    );

    let config = Config {
        trees: vec![tree.clone()],
        platforms,
        fixups: Default::default(),
        buck: Default::default(),
        lockfile: Default::default(),
    };

    // typing_extensions is added on py311 only, dropped on py312.
    let marker_py311_only: MarkerTree = MarkerTree::from_str("python_version < '3.12'").unwrap();

    let lockfile = Lockfile {
        version: 1,
        revision: 3,
        requires_python: ">=3.11,<3.13".into(),
        packages: vec![
            // first-party root
            Package {
                name: PackageName::from_str("app").unwrap(),
                version: Version::from_str("0.1").unwrap(),
                source: Source::FirstParty { kind: FirstPartyKind::Virtual, path: ".".into() },
                dependencies: vec![DepEdge {
                    name: PackageName::from_str("rich").unwrap(),
                    extra: vec![],
                    marker: None,
                }],
                sdist: None,
                wheels: vec![],
                metadata: None,
            },
            // rich: depends on typing_extensions for py311 only
            Package {
                name: PackageName::from_str("rich").unwrap(),
                version: Version::from_str("13.0").unwrap(),
                source: Source::Registry { url: Url::parse("https://pypi.org/simple").unwrap() },
                dependencies: vec![
                    DepEdge {
                        name: PackageName::from_str("typing-extensions").unwrap(),
                        extra: vec![],
                        marker: Some(marker_py311_only),
                    },
                ],
                sdist: None,
                wheels: vec![Wheel {
                    url: Url::parse("https://files.pythonhosted.org/p/rich-13.0-py3-none-any.whl").unwrap(),
                    hash: "sha256:rich".into(),
                    size: None,
                    filename: "rich-13.0-py3-none-any.whl".into(),
                }],
                metadata: None,
            },
            // typing-extensions
            Package {
                name: PackageName::from_str("typing-extensions").unwrap(),
                version: Version::from_str("4.0").unwrap(),
                source: Source::Registry { url: Url::parse("https://pypi.org/simple").unwrap() },
                dependencies: vec![],
                sdist: None,
                wheels: vec![Wheel {
                    url: Url::parse("https://files.pythonhosted.org/p/typing_extensions-4.0-py3-none-any.whl").unwrap(),
                    hash: "sha256:te".into(),
                    size: None,
                    filename: "typing_extensions-4.0-py3-none-any.whl".into(),
                }],
                metadata: None,
            },
        ],
    };

    let input = build_emit_input(&config, &tree, &lockfile).expect("succeeds");

    let rich = input.packages.iter().find(|p| p.name == "rich").expect("rich present");
    match &rich.deps {
        EmitDeps::PerCell(m) => {
            // py311 cell has typing-extensions; py312 cell does not.
            let py311 = ConfigName::new("3.11", "linux-x86_64-gnu");
            let py312 = ConfigName::new("3.12", "linux-x86_64-gnu");
            assert_eq!(m.get(&py311).unwrap(), &vec![":typing-extensions".to_string()]);
            assert_eq!(m.get(&py312).unwrap(), &Vec::<String>::new());
        }
        EmitDeps::Uniform(_) => panic!("expected PerCell, got Uniform"),
    }
}
```

If `MarkerTree::from_str` is not how this codebase constructs markers, look at `src/lock/types.rs` for the actual marker type and adapt.

- [ ] **Step 3: Run — expect fail**

```bash
cargo test --lib buck::emit::tests::build_emit_input_produces_per_cell_when_deps_differ 2>&1 | tail -10
```

Expected: FAIL — current code either (a) bails with the cross-cell-mismatch error, or (b) returns `Uniform` because Task 7's stub wrapped deps in Uniform unconditionally.

- [ ] **Step 4: Write a failing test for Uniform collapse**

```rust
#[test]
fn build_emit_input_collapses_to_uniform_when_cells_agree() {
    // Reuse the build_emit_input_from_synthetic_resolved fixture (certifi with
    // no deps). Both cells should produce the same empty deps list -> Uniform.
    // ... (copy the body of build_emit_input_from_synthetic_resolved but
    //      assert via match that deps is EmitDeps::Uniform(empty))
    // ... and add a second python version so it's a multi-cell test
}
```

Concrete: copy the existing `build_emit_input_from_synthetic_resolved`'s `tree`/`config`/`lockfile` definitions but set `python_versions: vec![PythonVersion(3, 11), PythonVersion(3, 12)]`. Then:

```rust
match &pkg.deps {
    EmitDeps::Uniform(v) => assert!(v.is_empty()),
    EmitDeps::PerCell(_) => panic!("expected Uniform collapse"),
}
```

Run:

```bash
cargo test --lib buck::emit::tests::build_emit_input_collapses_to_uniform_when_cells_agree 2>&1 | tail -5
```

Expected: PASS (Task 7's stub returns `Uniform` unconditionally — this test passes already). That's fine — it locks the contract.

- [ ] **Step 5: Drop the `bail!()` and add collapse-uniform**

In `src/buck/emit.rs::build_emit_input`, replace lines 178-213 (the cross-cell-mismatch bail + sort/dedup + `packages.push`) with:

```rust
// Cross-cell dep equality check + build EmitPackage list.
let mut packages: Vec<EmitPackage> = Vec::new();
for (key, wheel_map) in pkg_wheels {
    let cells_deps = &pkg_deps_per_cell[&key];

    // Format each cell's deps: ResolvedPackage.deps entries are "name@version".
    // Extract bare name, prepend ':', sort, dedup.
    let format_cell_deps = |raw: &Vec<String>| -> Vec<String> {
        let mut v: Vec<String> = raw
            .iter()
            .map(|d| format!(":{}", d.split('@').next().unwrap_or(d)))
            .collect();
        v.sort();
        v.dedup();
        v
    };

    let mut per_cell_formatted: BTreeMap<ConfigName, Vec<String>> = BTreeMap::new();
    for (cell, raw) in cells_deps {
        per_cell_formatted.insert(cell.clone(), format_cell_deps(raw));
    }

    // Collapse: if all cells produce the same Vec<String>, render Uniform.
    let mut values_iter = per_cell_formatted.values();
    let first = values_iter
        .next()
        .expect("at least one cell recorded a wheel for this package");
    let uniform = values_iter.all(|v| v == first);

    let deps = if uniform {
        EmitDeps::Uniform(first.clone())
    } else {
        EmitDeps::PerCell(per_cell_formatted)
    };

    packages.push(EmitPackage {
        name: key.0,
        version: key.1,
        deps,
        wheels: wheel_map,
    });
}

packages.sort_by(|a, b| a.name.cmp(&b.name).then(a.version.cmp(&b.version)));
```

The closure `format_cell_deps` exists to keep the (sort, dedup, format) logic DRY between cells; if a single inline expression is clearer, inline it.

- [ ] **Step 6: Delete the old bail!()-related code**

Confirm the S3 lines 178-194 (the for-loop that called `bail!()` on `deps != first_deps`) are gone — they were replaced in Step 5. Likewise the old `mut deps: Vec<String> = first_deps.iter().map(...)` block.

- [ ] **Step 7: Run — both new tests pass**

```bash
cargo test --lib buck::emit::tests::build_emit_input_produces_per_cell_when_deps_differ buck::emit::tests::build_emit_input_collapses_to_uniform_when_cells_agree 2>&1 | tail -10
```

Expected: PASS for both.

- [ ] **Step 8: Flip the old `build_emit_input_errors_on_dep_set_cross_cell_mismatch` test**

The S3 test asserts the bail happens. Now the same input should produce `PerCell`. Rename + flip:

```rust
#[test]
fn build_emit_input_yields_per_cell_for_cross_cell_dep_diff() {
    // Same setup that S3 used to trigger the bail; now it succeeds.
    // ... (lift the body of the old test)
    let input = build_emit_input(&config, &tree, &lockfile).expect("succeeds");
    let pkg = input.packages.iter().find(|p| p.name == "<the test package>")
        .expect("present");
    match &pkg.deps {
        EmitDeps::PerCell(m) => assert!(m.len() >= 2),  // at least two cells
        EmitDeps::Uniform(_) => panic!("expected PerCell"),
    }
}
```

Update the test name + assertions; the input fixture stays the same.

- [ ] **Step 9: Run full emit tests**

```bash
cargo test --lib buck::emit:: 2>&1 | tail -10
```

Expected: all emit tests pass (~10 tests).

- [ ] **Step 10: Run full suite**

```bash
cargo test 2>&1 | tail -5
```

Expected: 145 (S3 baseline) + 2 new (EmitDeps variants test + PerCell preservation test + Uniform collapse test) - 1 (the flipped/renamed test counts as the same test, not a new one) = 147. Confirm with the actual number printed.

- [ ] **Step 11: Commit**

```bash
git add src/buck/emit.rs
git commit -m "feat(s4): build_emit_input renders per-cell deps as PerCell variant

Replaces the S3 cross-cell-mismatch bail!() with a collapse-uniform
pass: per-cell deps are computed, then merged to Uniform if all
cells agree, else preserved as PerCell. Renames the flipped test
build_emit_input_yields_per_cell_for_cross_cell_dep_diff."
```

---

## Phase 3 — Emitter: multi-cell rendering

Phase 3 makes `string_writer.rs` render the four output files in their S4 shape. Each file gets its own task.

### Task 9: Render `deps = [...]` for `Uniform` (existing form, restored)

**Files:**
- Modify: `src/buck/string_writer.rs`

This task replaces the Task-7 stub (which returned an empty Vec for both variants) with the correct Uniform rendering. PerCell still stubs out to "EmitDeps::PerCell unsupported" — Task 10 fills that in.

- [ ] **Step 1: Read current rendering of `deps`**

```bash
grep -n "deps" src/buck/string_writer.rs | head -20
```

Find the function that renders a `pypi_package` call (likely `write_pypi_package_call`, `emit_package`, or similar).

- [ ] **Step 2: Restore Uniform rendering**

Locate the section that currently has the Task-7 stub `Vec<&String> = match &pkg.deps { ... }`. Replace with explicit branching:

```rust
match &pkg.deps {
    EmitDeps::Uniform(v) => {
        // existing rendering: deps = ["...", "..."]
        writeln!(buf, "    deps = [")?;
        for d in v {
            writeln!(buf, "        \"{}\",", d)?;
        }
        writeln!(buf, "    ],")?;
    }
    EmitDeps::PerCell(_) => {
        // Task 10 fills this in.
        unimplemented!("EmitDeps::PerCell rendering — Task 10");
    }
}
```

(Adjust formatting to match whatever S3 actually emits — pull from the existing `01-pure-python/expected/BUCK` to confirm exact whitespace and bracket style. If S3 had `deps = [":a", ":b"]` on one line for short lists, preserve that.)

- [ ] **Step 3: Verify existing 01-pure-python snapshot still passes**

```bash
cargo test --test buckify fixture_01_pure_python 2>&1 | tail -10
```

Expected: PASS. The Uniform path renders the same bytes as S3.

- [ ] **Step 4: Verify the `EmitDeps::PerCell` case panics (controlled regression)**

There's no test exercising PerCell yet at the rendering level. We add it in Task 10. For now confirm `unimplemented!()` is unreachable from existing tests:

```bash
cargo test 2>&1 | grep -E "FAIL|PASS|error" | head -20
```

Expected: no panics, all existing tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/buck/string_writer.rs
git commit -m "refactor(s4): restore Uniform deps rendering in string_writer

Replaces the Task-7 stub with explicit Uniform/PerCell match.
Uniform branch renders bytes identical to S3. PerCell branch stubs
to unimplemented!() — Task 10 fills it in. Existing 01-pure-python
snapshot still passes."
```

---

### Task 10: Render `deps = select({...})` for `PerCell`

**Files:**
- Modify: `src/buck/string_writer.rs`

- [ ] **Step 1: Write a failing snapshot test**

In `src/buck/string_writer.rs::tests`, add:

```rust
#[test]
fn renders_per_cell_deps_as_select() {
    use crate::buck::emit::{EmitDeps, EmitInput, EmitPackage, EmitWheel, ConfigName};

    let cells: Vec<ConfigName> = vec![
        ConfigName::new("3.11", "linux-x86_64-gnu"),
        ConfigName::new("3.12", "linux-x86_64-gnu"),
    ];

    let mut per_cell: BTreeMap<ConfigName, Vec<String>> = BTreeMap::new();
    per_cell.insert(cells[0].clone(), vec![":foo".into(), ":typing-extensions".into()]);
    per_cell.insert(cells[1].clone(), vec![":foo".into()]);

    let mut wheels: BTreeMap<ConfigName, EmitWheel> = BTreeMap::new();
    for cell in &cells {
        wheels.insert(cell.clone(), EmitWheel {
            url: format!("https://example.com/rich-13.0-{}.whl", cell),
            hash: "sha256:rich".into(),
        });
    }

    let input = EmitInput {
        tree: "default".into(),
        third_party_dir: "third-party/python".into(),
        configs: cells,
        packages: vec![EmitPackage {
            name: "rich".into(),
            version: "13.0".into(),
            deps: EmitDeps::PerCell(per_cell),
            wheels,
        }],
    };

    let out = StringTemplateEmitter.emit(&input);
    insta::assert_snapshot!("per_cell_deps_select", out.buck);
}
```

- [ ] **Step 2: Run — verify panics**

```bash
cargo test --lib buck::string_writer::tests::renders_per_cell_deps_as_select 2>&1 | tail -10
```

Expected: PANIC at the `unimplemented!()` from Task 9.

- [ ] **Step 3: Implement PerCell rendering**

Replace the `EmitDeps::PerCell(_)` arm:

```rust
EmitDeps::PerCell(per_cell) => {
    writeln!(buf, "    deps = select({{")?;
    for (cell, deps_v) in per_cell {
        // select() branch keys are config target labels
        let key = format!("//{}/config:{}", input.third_party_dir, cell);
        if deps_v.is_empty() {
            writeln!(buf, "        \"{}\": [],", key)?;
        } else {
            writeln!(buf, "        \"{}\": [", key)?;
            for d in deps_v {
                writeln!(buf, "            \"{}\",", d)?;
            }
            writeln!(buf, "        ],")?;
        }
    }
    writeln!(buf, "    }}),")?;
}
```

Note: `BTreeMap` iteration is already sorted by `ConfigName`, which is sorted lex — so select() branches come out in deterministic order without an extra sort.

- [ ] **Step 4: Run — snapshot accepts (initial)**

```bash
cargo test --lib buck::string_writer::tests::renders_per_cell_deps_as_select 2>&1 | tail -10
```

Expected: FAIL with "no snapshot, .new file written." Inspect:

```bash
cat src/buck/snapshots/buck__string_writer__tests__per_cell_deps_select.snap.new
```

It should show the `deps = select({...})` block correctly formatted. Visually confirm:
- Branches sorted by ConfigName lex.
- Each branch's key is `"//third-party/python/config:<ConfigName>"`.
- Each branch's value is `[":..."]` form, with `[]` for empty.

If it looks right, accept:

```bash
INSTA_UPDATE=always cargo test --lib buck::string_writer::tests::renders_per_cell_deps_as_select
```

- [ ] **Step 5: Run again to confirm**

```bash
cargo test --lib buck::string_writer::tests::renders_per_cell_deps_as_select 2>&1 | tail -5
```

Expected: PASS.

- [ ] **Step 6: Add a Uniform regression snapshot test (defensive)**

To guard against future drift between Uniform and PerCell rendering:

```rust
#[test]
fn renders_uniform_deps_as_plain_list() {
    use crate::buck::emit::{EmitDeps, EmitInput, EmitPackage, EmitWheel, ConfigName};

    let cell = ConfigName::new("3.12", "linux-x86_64-gnu");
    let mut wheels: BTreeMap<ConfigName, EmitWheel> = BTreeMap::new();
    wheels.insert(cell.clone(), EmitWheel {
        url: "https://example.com/requests-2.32.3-py3-none-any.whl".into(),
        hash: "sha256:req".into(),
    });

    let input = EmitInput {
        tree: "default".into(),
        third_party_dir: "third-party/python".into(),
        configs: vec![cell],
        packages: vec![EmitPackage {
            name: "requests".into(),
            version: "2.32.3".into(),
            deps: EmitDeps::Uniform(vec![":certifi".into(), ":idna".into()]),
            wheels,
        }],
    };

    let out = StringTemplateEmitter.emit(&input);
    insta::assert_snapshot!("uniform_deps_plain_list", out.buck);
}
```

```bash
cargo test --lib buck::string_writer::tests::renders_uniform_deps_as_plain_list 2>&1 | tail -10
INSTA_UPDATE=always cargo test --lib buck::string_writer::tests::renders_uniform_deps_as_plain_list
cargo test --lib buck::string_writer::tests::renders_uniform_deps_as_plain_list 2>&1 | tail -5
```

Expected after accept: PASS.

- [ ] **Step 7: Commit**

```bash
git add src/buck/string_writer.rs src/buck/snapshots/
git commit -m "feat(s4): render PerCell deps as select() with one branch per cell

select() branches sorted by ConfigName lex (BTreeMap iteration).
Empty cells render as []; non-empty as multi-line lists. Snapshot
tests cover both Uniform and PerCell rendering."
```

---

### Task 11: Render python-axis wiring header in `muntjac.bzl`

**Files:**
- Modify: `src/buck/string_writer.rs`

The header is a comment block above the existing `_CONFIGS` declaration. It needs to dynamically reflect the tree's `python_versions` (e.g. "Available muntjac python constraints: py311, py312").

- [ ] **Step 1: Write a failing snapshot test**

```rust
#[test]
fn muntjac_bzl_has_python_axis_wiring_header() {
    use crate::buck::emit::{EmitDeps, EmitInput, ConfigName};

    let input = EmitInput {
        tree: "default".into(),
        third_party_dir: "third-party/python".into(),
        configs: vec![
            ConfigName::new("3.11", "linux-x86_64-gnu"),
            ConfigName::new("3.12", "linux-x86_64-gnu"),
        ],
        packages: vec![],
    };

    let out = StringTemplateEmitter.emit(&input);
    insta::assert_snapshot!("muntjac_bzl_with_header", out.muntjac_bzl);
}
```

- [ ] **Step 2: Run — verify (snapshot will not exist)**

```bash
cargo test --lib buck::string_writer::tests::muntjac_bzl_has_python_axis_wiring_header 2>&1 | tail -5
```

Expected: FAIL with snapshot missing.

- [ ] **Step 3: Implement the header rendering**

In the function that produces `muntjac.bzl` (search `string_writer.rs` for `_CONFIGS` or `muntjac_bzl`), prepend a header block before the existing body:

```rust
fn write_muntjac_bzl(input: &EmitInput, buf: &mut String) -> Result<(), fmt::Error> {
    writeln!(buf, "##")?;
    writeln!(buf, "## @generated by muntjac")?;
    writeln!(buf, "##")?;
    writeln!(buf, "## Wiring contract for consumers:")?;
    writeln!(buf, "##")?;
    writeln!(buf, "##   Host OS + CPU is auto-wired in <third_party_dir>/PACKAGE. To pick a")?;
    writeln!(buf, "##   python version, set a per-binary modifier or a root-PACKAGE default:")?;
    writeln!(buf, "##")?;
    writeln!(buf, "##     # per-binary")?;
    writeln!(buf, "##     python_binary(")?;
    writeln!(buf, "##         name = \"service\",")?;
    writeln!(buf, "##         modifiers = [\"//{}/config:py312\"],", input.third_party_dir)?;
    writeln!(buf, "##         deps = [\"//{}:numpy\"],", input.third_party_dir)?;
    writeln!(buf, "##         main = \"main.py\",")?;
    writeln!(buf, "##     )")?;
    writeln!(buf, "##")?;
    writeln!(buf, "##     # root PACKAGE default")?;
    writeln!(buf, "##     load(\"@prelude//cfg/modifier:set_cfg_modifiers.bzl\", \"set_cfg_modifiers\")")?;
    writeln!(buf, "##     set_cfg_modifiers([\"//{}/config:py312\"])", input.third_party_dir)?;
    writeln!(buf, "##")?;

    // Available python constraints (dynamically derived from configs):
    let py_constraints: BTreeSet<String> = input.configs.iter()
        .map(|c| {
            // ConfigName is "py3XX-<platform>"; extract the "py3XX" prefix
            c.as_str().split('-').next().unwrap_or("").to_string()
        })
        .filter(|s| s.starts_with("py"))
        .collect();
    let py_list: Vec<&String> = py_constraints.iter().collect();
    writeln!(buf, "## Available muntjac python constraints: {}", py_list.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", "))?;
    writeln!(buf, "##")?;
    writeln!(buf)?;  // blank line before the body

    // ... existing body: load() calls, _CONFIGS, def pypi_package ...
}
```

(If your `BTreeSet` import isn't there: `use std::collections::BTreeSet;` near other imports.)

- [ ] **Step 4: Accept the new snapshot**

```bash
cargo test --lib buck::string_writer::tests::muntjac_bzl_has_python_axis_wiring_header 2>&1 | tail -10
cat src/buck/snapshots/buck__string_writer__tests__muntjac_bzl_with_header.snap.new
```

Visually verify: header is present, python constraints list reflects the input cells, `third_party_dir` substituted correctly.

```bash
INSTA_UPDATE=always cargo test --lib buck::string_writer::tests::muntjac_bzl_has_python_axis_wiring_header
cargo test --lib buck::string_writer::tests::muntjac_bzl_has_python_axis_wiring_header 2>&1 | tail -5
```

Expected after accept: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/buck/string_writer.rs src/buck/snapshots/
git commit -m "feat(s4): emit python-axis wiring header in muntjac.bzl

Generated muntjac.bzl now opens with a comment block documenting the
per-binary modifier snippet and root-PACKAGE default snippet for
picking a python version. Available python constraints are derived
dynamically from EmitInput.configs."
```

---

### Task 12: Render real `PACKAGE` with `set_cfg_modifiers` host-axis wiring

**Files:**
- Modify: `src/buck/string_writer.rs`

The S3 PACKAGE was a placeholder. S4 emits a real `set_cfg_modifiers` call with the host OS+CPU wiring. **Use the cell prefix validated by the Phase 1 spike** — the snippet below uses `config//` but adjust to match `scratch-s4-spike.md`.

- [ ] **Step 1: Re-read the validated PACKAGE shape from spike notes**

```bash
grep -A 20 "Validated set_cfg_modifiers" docs/superpowers/scratch-s4-spike.md
```

Use those exact load paths and constraint keys in the implementation below.

- [ ] **Step 2: Write a failing snapshot test**

```rust
#[test]
fn package_file_wires_host_axis() {
    use crate::buck::emit::{EmitInput, ConfigName};

    let input = EmitInput {
        tree: "default".into(),
        third_party_dir: "third-party/python".into(),
        configs: vec![
            ConfigName::new("3.12", "linux-aarch64-gnu"),
            ConfigName::new("3.12", "linux-x86_64-gnu"),
            ConfigName::new("3.12", "macos-arm64"),
        ],
        packages: vec![],
    };

    let out = StringTemplateEmitter.emit(&input);
    insta::assert_snapshot!("package_three_platforms", out.package_file);
}
```

- [ ] **Step 3: Implement PACKAGE rendering**

Find the existing PACKAGE writer (likely `write_package_file` returning the `package_file` field). Replace its body with:

```rust
fn write_package_file(input: &EmitInput, buf: &mut String) -> Result<(), fmt::Error> {
    writeln!(buf, "##")?;
    writeln!(buf, "## @generated by muntjac")?;
    writeln!(buf, "## Do not edit by hand.")?;
    writeln!(buf, "##")?;
    writeln!(buf)?;
    writeln!(buf, "load(\"@prelude//cfg/modifier:set_cfg_modifiers.bzl\", \"set_cfg_modifiers\")")?;
    writeln!(buf)?;
    writeln!(buf, "# Bind the build's host OS+CPU to muntjac's platform constraint.")?;
    writeln!(buf, "# Python version is picked by the user via a per-binary `modifiers` attr")?;
    writeln!(buf, "# or a root PACKAGE default; see muntjac.bzl header for the snippet.")?;
    writeln!(buf)?;

    // Derive platforms from configs (strip "py3XX-" prefix).
    // E.g. "py312-linux-x86_64-gnu" -> "linux-x86_64-gnu".
    let mut platforms: BTreeSet<&str> = BTreeSet::new();
    for cfg in &input.configs {
        if let Some(plat) = cfg.as_str().splitn(2, '-').nth(1) {
            platforms.insert(plat);
        }
    }

    // Group platforms by OS family for nested ModifiersMatch.
    // <os> => Vec<(cpu, full_platform_name)>
    let mut by_os: BTreeMap<&str, Vec<(&str, &str)>> = BTreeMap::new();
    for p in &platforms {
        let parts: Vec<&str> = p.split('-').collect();
        // expected shape: "linux-x86_64-gnu" -> os="linux", cpu="x86_64"
        //                 "linux-aarch64-gnu" -> os="linux", cpu="aarch64"  
        //                 "macos-arm64" -> os="macos", cpu="arm64"
        if parts.len() >= 2 {
            by_os.entry(parts[0]).or_default().push((parts[1], p));
        }
    }

    writeln!(buf, "set_cfg_modifiers(")?;
    writeln!(buf, "    cfg_modifiers = [")?;
    writeln!(buf, "        {{")?;  // outer ModifiersMatch
    for (os, cpus) in &by_os {
        let os_key = format!("<spike-validated-prefix>//os/constraints:{}", os);  // CHECK SCRATCH NOTES
        writeln!(buf, "            \"{}\": {{", os_key)?;
        for (cpu, plat_name) in cpus {
            let cpu_key = format!("<spike-validated-prefix>//cpu/constraints:{}", cpu);  // CHECK SCRATCH NOTES
            let target = format!("//{}/config:{}", input.third_party_dir, plat_name);
            writeln!(buf, "                \"{}\": \"{}\",", cpu_key, target)?;
        }
        writeln!(buf, "            }},")?;
    }
    writeln!(buf, "        }},")?;
    writeln!(buf, "    ],")?;
    writeln!(buf, ")")?;
    Ok(())
}
```

**Critical:** Replace `<spike-validated-prefix>` with the actual cell prefix from `scratch-s4-spike.md`. If the spike found that OS+CPU keys belong in a flat (non-nested) dict instead, restructure accordingly. The general principle: emit whatever shape the spike validated.

- [ ] **Step 4: Accept the snapshot**

```bash
cargo test --lib buck::string_writer::tests::package_file_wires_host_axis 2>&1 | tail -10
cat src/buck/snapshots/buck__string_writer__tests__package_three_platforms.snap.new
```

Visually confirm: load() path correct; OS dict has linux + macos; linux dict has x86_64 + aarch64; macos dict has arm64; targets reference `//third-party/python/config:<platform>`.

```bash
INSTA_UPDATE=always cargo test --lib buck::string_writer::tests::package_file_wires_host_axis
cargo test --lib buck::string_writer::tests::package_file_wires_host_axis 2>&1 | tail -5
```

Expected: PASS.

- [ ] **Step 5: Verify the rendered PACKAGE actually compiles under buck2**

Compare the rendered PACKAGE byte-for-byte against the hand-written PACKAGE that worked in the Phase 1 spike. They should match (modulo whitespace). If they don't, **run `buck2 run //tests/smoke:numpy_demo`** from the fixture with the *rendered* PACKAGE (write it out manually for now, or via the next task's pipeline). If buck2 errors, fix the renderer.

This is critical — getting the rendering wrong here is what the §10 spike was supposed to prevent.

```bash
# Copy rendered PACKAGE into the fixture's generated dir, run buck2:
cd tests/fixtures/buck/02-numpy-pandas
# (manual copy from snapshot or programmatic)
buck2 clean
buck2 run //tests/smoke:numpy_demo 2>&1 | tail -5
```

Expected: `[0. 0. 0.]` printed.

If the buck2 run fails, **read the error**, adjust the renderer in Step 3 to produce the shape buck2 actually wants, re-accept the snapshot (`INSTA_UPDATE=always`), and re-test.

- [ ] **Step 6: Commit**

```bash
git add src/buck/string_writer.rs src/buck/snapshots/
git commit -m "feat(s4): emit real PACKAGE with set_cfg_modifiers host-axis wiring

S3's placeholder PACKAGE is replaced with a real set_cfg_modifiers
call. Nested ModifiersMatch dict binds host OS+CPU -> muntjac's
platform constraint. Cell prefix from the Phase 1 spike validation.
Python axis stays user-side (per-binary modifiers attr; see muntjac.bzl
header)."
```

---

### Task 13: Extend `config/BUCK` for multi-platform constraint_values + multi-cell config_settings

**Files:**
- Modify: `src/buck/string_writer.rs`

S3 emitted constraint_values for one platform (whatever single platform was in muntjac.toml). S4 needs to emit one constraint_value per platform + one config_setting per cell.

- [ ] **Step 1: Read the current config/BUCK rendering**

```bash
grep -n "constraint_value\|config_setting" src/buck/string_writer.rs | head -20
```

Find the function that produces the `config_buck` field (likely `write_config_buck`).

- [ ] **Step 2: Write a failing snapshot test**

```rust
#[test]
fn config_buck_emits_multi_platform_constraints() {
    use crate::buck::emit::{EmitInput, ConfigName};

    let input = EmitInput {
        tree: "default".into(),
        third_party_dir: "third-party/python".into(),
        configs: vec![
            ConfigName::new("3.11", "linux-aarch64-gnu"),
            ConfigName::new("3.11", "linux-x86_64-gnu"),
            ConfigName::new("3.11", "macos-arm64"),
            ConfigName::new("3.12", "linux-aarch64-gnu"),
            ConfigName::new("3.12", "linux-x86_64-gnu"),
            ConfigName::new("3.12", "macos-arm64"),
        ],
        packages: vec![],
    };

    let out = StringTemplateEmitter.emit(&input);
    insta::assert_snapshot!("config_buck_six_cells", out.config_buck);
}
```

- [ ] **Step 3: Run — verify (snapshot missing)**

```bash
cargo test --lib buck::string_writer::tests::config_buck_emits_multi_platform_constraints 2>&1 | tail -5
```

Expected: FAIL with snapshot missing.

- [ ] **Step 4: Implement multi-axis rendering**

Replace the config_buck writer:

```rust
fn write_config_buck(input: &EmitInput, buf: &mut String) -> Result<(), fmt::Error> {
    writeln!(buf, "##")?;
    writeln!(buf, "## @generated by muntjac")?;
    writeln!(buf, "## Do not edit by hand.")?;
    writeln!(buf, "##")?;
    writeln!(buf)?;

    // Derive axes from configs.
    let mut pythons: BTreeSet<&str> = BTreeSet::new();
    let mut platforms: BTreeSet<&str> = BTreeSet::new();
    for cfg in &input.configs {
        // Tokenize "py3XX-<platform>". The "py3XX" prefix is the first '-'-delimited segment.
        let s = cfg.as_str();
        if let Some(dash) = s.find('-') {
            pythons.insert(&s[..dash]);
            platforms.insert(&s[dash + 1..]);
        }
    }

    // python_version axis
    writeln!(buf, "constraint_setting(name = \"python_version\")")?;
    for py in &pythons {
        writeln!(buf, "constraint_value(name = \"{}\", constraint_setting = \":python_version\")", py)?;
    }
    writeln!(buf)?;

    // platform axis
    writeln!(buf, "constraint_setting(name = \"platform\")")?;
    for plat in &platforms {
        writeln!(buf, "constraint_value(name = \"{}\", constraint_setting = \":platform\")", plat)?;
    }
    writeln!(buf)?;

    // config_settings — one per cell (= one per ConfigName in input.configs)
    for cell in &input.configs {
        let s = cell.as_str();
        let dash = s.find('-').expect("ConfigName format py<X>-<platform>");
        let py = &s[..dash];
        let plat = &s[dash + 1..];

        // sort constraint_values lex (platform before python alphabetically)
        let mut cvs = vec![plat, py];
        cvs.sort();

        writeln!(buf, "config_setting(")?;
        writeln!(buf, "    name = \"{}\",", cell)?;
        writeln!(buf, "    constraint_values = [")?;
        for cv in &cvs {
            writeln!(buf, "        \":{}\",", cv)?;
        }
        writeln!(buf, "    ],")?;
        writeln!(buf, ")")?;
    }

    Ok(())
}
```

- [ ] **Step 5: Accept the snapshot**

```bash
cargo test --lib buck::string_writer::tests::config_buck_emits_multi_platform_constraints 2>&1 | tail -10
cat src/buck/snapshots/buck__string_writer__tests__config_buck_six_cells.snap.new
```

Visually verify:
- `constraint_setting(name = "python_version")` followed by `py311, py312` alphabetic.
- `constraint_setting(name = "platform")` followed by `linux-aarch64-gnu, linux-x86_64-gnu, macos-arm64` alphabetic.
- 6 config_settings, sorted by name lex.
- Each config_setting's `constraint_values` lists platform first, then python (alphabetic).

```bash
INSTA_UPDATE=always cargo test --lib buck::string_writer::tests::config_buck_emits_multi_platform_constraints
cargo test --lib buck::string_writer::tests::config_buck_emits_multi_platform_constraints 2>&1 | tail -5
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src/buck/string_writer.rs src/buck/snapshots/
git commit -m "feat(s4): emit per-axis constraint_values + per-cell config_settings

config/BUCK now emits two constraint_settings (python_version,
platform), one constraint_value per python and per platform, and
one config_setting per cell in EmitInput.configs. All sorted lex.
Snapshot covers the 6-cell case (3 platforms x 2 pythons)."
```

---

## Phase 4 — Fixtures: goldens + integration tests

### Task 14: Generate `02-numpy-pandas/expected/` goldens

**Files:**
- Create: `tests/fixtures/buck/02-numpy-pandas/expected/BUCK`
- Create: `tests/fixtures/buck/02-numpy-pandas/expected/muntjac.bzl`
- Create: `tests/fixtures/buck/02-numpy-pandas/expected/PACKAGE`
- Create: `tests/fixtures/buck/02-numpy-pandas/expected/config/BUCK`

The goldens are generated by running buckify against the fixture, then committing the output to `expected/`.

- [ ] **Step 1: Run buckify against the fixture**

```bash
cd tests/fixtures/buck/02-numpy-pandas
cargo run --release -- buckify 2>&1 | tail -10
ls -la third-party/python/
```

Expected: four files in `third-party/python/` — `BUCK`, `muntjac.bzl`, `PACKAGE`, `config/BUCK`.

- [ ] **Step 2: Verify the generated PACKAGE works under buck2**

```bash
buck2 clean
buck2 run //tests/smoke:numpy_demo 2>&1 | tail -5
```

Expected: `[0. 0. 0.]` printed.

If it fails: the emitter is producing a different PACKAGE shape than the hand-written one from Phase 1. **Fix the emitter** (Task 12 / Task 13 may need adjustments), don't fix the goldens.

- [ ] **Step 3: Copy the generated files into expected/**

```bash
cd /home/jackm/repos/muntjac/tests/fixtures/buck/02-numpy-pandas
mkdir -p expected/config
cp third-party/python/BUCK expected/BUCK
cp third-party/python/muntjac.bzl expected/muntjac.bzl
cp third-party/python/PACKAGE expected/PACKAGE
cp third-party/python/config/BUCK expected/config/BUCK
```

- [ ] **Step 4: Eyeball the goldens**

```bash
ls -la expected/ expected/config/
wc -l expected/BUCK expected/muntjac.bzl expected/PACKAGE expected/config/BUCK
head -30 expected/BUCK
head -30 expected/muntjac.bzl
cat expected/PACKAGE
head -30 expected/config/BUCK
```

Confirm:
- `expected/BUCK` has one `pypi_package(name = "numpy", ...)` call with 6 wheel entries.
- `expected/muntjac.bzl` has the python-axis wiring header + `_CONFIGS` with 6 entries.
- `expected/PACKAGE` has the host-axis `set_cfg_modifiers` call.
- `expected/config/BUCK` has 2 python constraint_values + 3 platform constraint_values + 6 config_settings.

Each file ends with a newline (POSIX text file convention). If not, append one before committing.

- [ ] **Step 5: Re-run buckify; verify determinism**

```bash
rm -rf third-party/python/
cargo run --release -- buckify 2>&1 | tail -3
diff -r third-party/python/ expected/
```

Expected: no diff. Confirms the emitter is byte-stable.

- [ ] **Step 6: Commit**

```bash
git add tests/fixtures/buck/02-numpy-pandas/expected/
git commit -m "test(s4): commit 02-numpy-pandas golden expected/ files

Generated by cargo run --release -- buckify on the frozen uv.lock.
Six cells (3 platforms x 2 pythons); numpy 2.1.<patch>; no transitive
runtime deps. Goldens validated by running buck2 run //tests/smoke:numpy_demo
against the generated files."
```

---

### Task 15: Integration test `fixture_02_numpy_pandas_golden`

**Files:**
- Modify: `tests/buckify.rs`

- [ ] **Step 1: Read the existing `fixture_01_pure_python_golden` for pattern**

```bash
grep -A 25 "fixture_01_pure_python" tests/buckify.rs
```

Copy the structure: copy fixture inputs into tempdir, run buckify, byte-compare against `expected/`.

- [ ] **Step 2: Add `fixture_02_numpy_pandas_golden`**

In `tests/buckify.rs`, add (adapting from the 01 helper):

```rust
#[test]
fn fixture_02_numpy_pandas_golden() {
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/buck/02-numpy-pandas");
    let tmp = tempfile::tempdir().unwrap();

    // Copy inputs (not expected/, not third-party/, not .git*)
    for name in ["muntjac.toml", "pyproject.toml", "uv.lock"] {
        std::fs::copy(fixture.join(name), tmp.path().join(name)).unwrap();
    }

    run_buckify(tmp.path()).expect("buckify succeeds");

    let third_party = tmp.path().join("third-party/python");
    for rel in ["BUCK", "muntjac.bzl", "PACKAGE", "config/BUCK"] {
        let actual = std::fs::read(third_party.join(rel))
            .unwrap_or_else(|_| panic!("missing generated file: {}", rel));
        let expected = std::fs::read(fixture.join("expected").join(rel))
            .unwrap_or_else(|_| panic!("missing expected file: {}", rel));
        assert_eq!(
            String::from_utf8_lossy(&actual),
            String::from_utf8_lossy(&expected),
            "{} mismatch",
            rel
        );
    }
}
```

(The exact `run_buckify` helper signature is whatever S3's `fixture_01_pure_python_golden` uses. If it returns `anyhow::Result<()>`, use `.expect()`.)

- [ ] **Step 3: Run — verify passes**

```bash
cargo test --test buckify fixture_02_numpy_pandas_golden 2>&1 | tail -10
```

Expected: PASS.

- [ ] **Step 4: Run all integration tests**

```bash
cargo test --test buckify 2>&1 | tail -10
```

Expected: 01-pure-python and 02-numpy-pandas both pass. Determinism test still passes.

- [ ] **Step 5: Commit**

```bash
git add tests/buckify.rs
git commit -m "test(s4): integration test fixture_02_numpy_pandas_golden

Asserts buckify on 02-numpy-pandas inputs produces byte-identical
output to expected/. Six-cell matrix (3 platforms x 2 pythons)
exercising the multi-platform emitter."
```

---

### Task 16: Generate `03-musllinux/expected/` goldens + integration test

**Files:**
- Create: `tests/fixtures/buck/03-musllinux/expected/BUCK`
- Create: `tests/fixtures/buck/03-musllinux/expected/muntjac.bzl`
- Create: `tests/fixtures/buck/03-musllinux/expected/PACKAGE`
- Create: `tests/fixtures/buck/03-musllinux/expected/config/BUCK`
- Modify: `tests/buckify.rs`

- [ ] **Step 1: Run buckify against the fixture**

```bash
cd tests/fixtures/buck/03-musllinux
cargo run --release -- -C $(pwd) buckify 2>&1 | tail -5
ls third-party/python/
```

Expected: four generated files.

- [ ] **Step 2: Verify musllinux wheel was picked**

```bash
grep -E "musllinux|whl" third-party/python/BUCK | head -5
```

Expected: at least one URL field contains `musllinux_1_2_x86_64`. If you see only `manylinux` or sdist refs, something's wrong — go back to Task 2 and reconsider the package choice.

- [ ] **Step 3: Copy generated files to expected/**

```bash
mkdir -p expected/config
cp third-party/python/BUCK expected/BUCK
cp third-party/python/muntjac.bzl expected/muntjac.bzl
cp third-party/python/PACKAGE expected/PACKAGE
cp third-party/python/config/BUCK expected/config/BUCK
```

- [ ] **Step 4: Add integration test `fixture_03_musllinux_golden`**

In `tests/buckify.rs`, add:

```rust
#[test]
fn fixture_03_musllinux_golden() {
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/buck/03-musllinux");
    let tmp = tempfile::tempdir().unwrap();

    for name in ["muntjac.toml", "pyproject.toml", "uv.lock"] {
        std::fs::copy(fixture.join(name), tmp.path().join(name)).unwrap();
    }

    run_buckify(tmp.path()).expect("buckify succeeds");

    let third_party = tmp.path().join("third-party/python");
    for rel in ["BUCK", "muntjac.bzl", "PACKAGE", "config/BUCK"] {
        let actual = std::fs::read(third_party.join(rel)).unwrap();
        let expected = std::fs::read(fixture.join("expected").join(rel)).unwrap();
        assert_eq!(
            String::from_utf8_lossy(&actual),
            String::from_utf8_lossy(&expected),
            "{} mismatch",
            rel
        );
    }

    // Sanity: at least one URL in BUCK contains a musllinux substring.
    let buck = std::fs::read_to_string(third_party.join("BUCK")).unwrap();
    assert!(
        buck.contains("musllinux"),
        "expected at least one musllinux wheel URL in generated BUCK"
    );
}
```

- [ ] **Step 5: Run — verify passes**

```bash
cargo test --test buckify fixture_03_musllinux_golden 2>&1 | tail -10
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add tests/fixtures/buck/03-musllinux/expected/ tests/buckify.rs
git commit -m "test(s4): 03-musllinux fixture goldens + integration test

Asserts the wheel selector picks *musllinux_1_2_x86_64* on a
linux-x86_64-musl cell. Emit-only verification — no buck2 build
(Linux GH runners are glibc, not musl)."
```

---

### Task 17: Regenerate `01-pure-python/expected/` goldens for new PACKAGE shape

**Files:**
- Modify: `tests/fixtures/buck/01-pure-python/expected/PACKAGE`
- Modify (maybe): `tests/fixtures/buck/01-pure-python/expected/muntjac.bzl` (header changes)
- Modify (maybe): `tests/fixtures/buck/01-pure-python/expected/config/BUCK` (constraint_values shape)

The S3 fixture's expected files reflected the placeholder PACKAGE and single-platform config/BUCK. S4 changes both. The other files (BUCK) should be unchanged.

- [ ] **Step 1: Run buckify against 01-pure-python and capture the diff**

```bash
cd tests/fixtures/buck/01-pure-python
rm -rf third-party/python/  # or whatever the third_party_dir is
cargo run --release -- -C $(pwd) buckify 2>&1 | tail -3
diff -r third-party/python/ expected/ | head -40
```

- [ ] **Step 2: Inspect each diff**

For each file that diffs, confirm the diff is *expected* per S4 changes:
- `PACKAGE`: should diff substantially (placeholder → real `set_cfg_modifiers`).
- `muntjac.bzl`: should gain the python-axis wiring header (Task 11).
- `config/BUCK`: should gain a `platform` constraint_setting + the host-platform constraint_value if the 01 fixture wasn't already declaring it.
- `BUCK`: should NOT diff (Uniform deps; existing rendering).

If `BUCK` diffs unexpectedly, something else changed — investigate before proceeding.

- [ ] **Step 3: Update the expected/ files**

```bash
cp third-party/python/PACKAGE expected/PACKAGE
cp third-party/python/muntjac.bzl expected/muntjac.bzl
cp third-party/python/config/BUCK expected/config/BUCK
# BUCK only if it diffed (it shouldn't, but to be safe):
diff third-party/python/BUCK expected/BUCK || cp third-party/python/BUCK expected/BUCK
```

- [ ] **Step 4: Re-run 01-pure-python integration test**

```bash
cd /home/jackm/repos/muntjac
cargo test --test buckify fixture_01_pure_python 2>&1 | tail -5
```

Expected: PASS.

- [ ] **Step 5: Run the full test suite**

```bash
cargo test 2>&1 | tail -10
```

Expected: all tests pass.

- [ ] **Step 6: Commit**

```bash
git add tests/fixtures/buck/01-pure-python/expected/
git commit -m "test(s4): regenerate 01-pure-python goldens for new PACKAGE shape

S4's PACKAGE auto-wiring and multi-axis config/BUCK shift bytes
even on single-platform fixtures. Generated by cargo run buckify
on the unchanged inputs; BUCK contents unchanged."
```

---

## Phase 5 — CI workflow

### Task 18: Add buck2 install step to `.github/workflows/ci.yml`

**Files:**
- Modify: `.github/workflows/ci.yml`

Use the install procedure validated in Phase 1 (Task 3). Pin the buck2 version exactly.

- [ ] **Step 1: Read current ci.yml**

```bash
cat .github/workflows/ci.yml
```

Note the current job structure: matrix over `runner`, steps in order (checkout, toolchain, cache, fmt, clippy, build, test).

- [ ] **Step 2: Add the buck2 install step**

After the `cargo test` step, add:

```yaml
      - name: install buck2
        run: |
          set -euo pipefail
          BUCK2_RELEASE="<pinned-tag-from-Task-3>"
          case "${{ matrix.runner }}" in
            ubuntu-latest)       ASSET="buck2-x86_64-unknown-linux-gnu.zst"   ;;
            ubuntu-24.04-arm)    ASSET="buck2-aarch64-unknown-linux-gnu.zst"  ;;
            macos-latest)        ASSET="buck2-aarch64-apple-darwin.zst"       ;;
            *) echo "unknown runner ${{ matrix.runner }}"; exit 1 ;;
          esac
          mkdir -p "$HOME/.local/bin"
          curl -L "https://github.com/facebook/buck2/releases/download/${BUCK2_RELEASE}/${ASSET}" \
              -o /tmp/buck2.zst
          # zstd may not be pre-installed on macOS GH runners; install if needed
          if ! command -v zstd >/dev/null 2>&1; then
            if [ "${{ matrix.runner }}" = "macos-latest" ]; then
              brew install zstd
            else
              sudo apt-get update && sudo apt-get install -y zstd
            fi
          fi
          zstd -d /tmp/buck2.zst -o "$HOME/.local/bin/buck2"
          chmod +x "$HOME/.local/bin/buck2"
          echo "$HOME/.local/bin" >> "$GITHUB_PATH"
          "$HOME/.local/bin/buck2" --version
```

Replace `<pinned-tag-from-Task-3>` with the actual tag you used in Task 3.

If the asset name conventions are different from what I drafted (e.g. macOS asset has a different triple), use what the release page actually shows. Verify against `gh release view <tag> --repo facebook/buck2 --json assets`.

- [ ] **Step 3: Commit**

```bash
git add .github/workflows/ci.yml
git commit -m "ci(s4): add buck2 install step pinned to <tag>

Installs buck2 to ~/.local/bin/buck2 on all three matrix runners.
Per-runner asset selection (x86_64-linux, aarch64-linux, aarch64-darwin).
zstd installed on-demand if missing. Version pinned because the
cfg-modifier prelude API is in active churn upstream."
```

---

### Task 19: Add muntjac buckify + buck2 run smoke step

**Files:**
- Modify: `.github/workflows/ci.yml`

After the buck2 install step, add steps to run buckify and execute the numpy demo.

- [ ] **Step 1: Add the smoke steps**

```yaml
      - name: muntjac buckify (02-numpy-pandas fixture)
        run: |
          cd tests/fixtures/buck/02-numpy-pandas
          # Initialize prelude submodule if not already done by checkout.
          git submodule update --init --recursive --depth 1
          cargo run --release --manifest-path ../../../../Cargo.toml -- buckify
          # Verify the four files exist.
          test -f third-party/python/BUCK
          test -f third-party/python/muntjac.bzl
          test -f third-party/python/PACKAGE
          test -f third-party/python/config/BUCK

      - name: buck2 build + run numpy demo
        run: |
          cd tests/fixtures/buck/02-numpy-pandas
          buck2 run //tests/smoke:numpy_demo 2>&1 | tee /tmp/numpy-out.txt
          # Sanity-check the output contains the expected array
          grep -F "[0. 0. 0.]" /tmp/numpy-out.txt
```

If the `cargo run --manifest-path` path is wrong, adjust to point at the muntjac repo's root `Cargo.toml`. From `tests/fixtures/buck/02-numpy-pandas/`, four levels up = repo root.

Note: if the checkout step doesn't fetch submodules by default, add `submodules: recursive` to the `actions/checkout@v4` step at the top:

```yaml
      - uses: actions/checkout@v4
        with:
          submodules: recursive
```

- [ ] **Step 2: Verify locally before pushing**

```bash
# Simulate the CI step locally
cd /home/jackm/repos/muntjac/tests/fixtures/buck/02-numpy-pandas
rm -rf third-party/python/
git submodule update --init --recursive --depth 1
cargo run --release --manifest-path ../../../../Cargo.toml -- buckify
buck2 run //tests/smoke:numpy_demo
```

Expected: `[0. 0. 0.]` printed.

- [ ] **Step 3: Commit**

```bash
git add .github/workflows/ci.yml
git commit -m "ci(s4): smoke step — buckify + buck2 run numpy_demo

After cargo test, each matrix runner:
1. Initializes the prelude submodule (depth=1 for speed).
2. Runs muntjac buckify on the 02-numpy-pandas fixture.
3. Asserts buck2 run //tests/smoke:numpy_demo prints [0. 0. 0.].

ubuntu-latest exercises py312-linux-x86_64-gnu cell;
ubuntu-24.04-arm exercises py312-linux-aarch64-gnu;
macos-latest exercises py312-macos-arm64."
```

---

### Task 20: Push to remote + verify CI green on all three runners

**Files:** none modified

This is a verification gate — push to a branch, watch CI, fix failures iteratively.

- [ ] **Step 1: Push the branch**

```bash
git push -u origin <branch-name>
```

(If working on main locally: create a branch first — `git checkout -b s4-multiplatform` — then push.)

- [ ] **Step 2: Watch CI**

```bash
gh run watch
# OR
gh run list --limit 1
gh run view <run-id> --log
```

Expected: three jobs running (ubuntu-latest, ubuntu-24.04-arm, macos-latest). Each goes through fmt/clippy/build/test (existing) + buck2 install + buckify + numpy demo (new).

- [ ] **Step 3: Fix any failures**

Common CI-specific issues:
- **submodule init fails**: `actions/checkout@v4` needs `submodules: recursive`.
- **buck2 install fails on aarch64-linux**: the asset name may be different. Check `gh release view <tag> --repo facebook/buck2 --json assets`.
- **zstd not installed**: handled in Task 18 but check the error.
- **macos-latest fails python toolchain detection**: macos-latest's preinstalled python may not be exposed where the prelude expects. May need to add a step `- run: brew install python@3.12` or set `PYTHON3=$(which python3)`.
- **buck2 cache misses lead to slow first runs**: expected; doesn't fail the job.

Iterate: push fix, watch CI, repeat. Each iteration is a commit (small fixes are OK to commit individually).

- [ ] **Step 4: Confirm all three runners green**

When all three matrix jobs pass:

```bash
gh run list --limit 1
# Look for: "success" status, ~all three jobs green
```

- [ ] **Step 5: No commit needed (CI verification is the gate)**

If iterations were needed, those commits already landed. This step is a checkpoint, not a new commit.

---

## Phase 6 — Tech-debt fold-ins

These three items are spec'd in §8 of the design and tracked in `TECH_DEBT.md`. They're independent of the S4 emitter changes and can land at any time during the stage.

### Task 21: Add `LockfileError::BadUrl` variant + migrate sdist URL parser

**Files:**
- Modify: `src/error.rs` (or wherever `LockfileError` is defined — check `src/lock/parser.rs` for imports)
- Modify: `src/lock/parser.rs`

- [ ] **Step 1: Locate `LockfileError`**

```bash
grep -rn "enum LockfileError" src/
```

Expected: a single definition in `src/error.rs` or `src/lock/error.rs`. Note the file.

- [ ] **Step 2: Read the BadVersion variant uses for URL errors**

```bash
grep -B2 -A4 "BadVersion" src/lock/parser.rs | head -40
```

Find the `BadVersion { ... reason: format!("wheel URL: ..." ... }` calls (and equivalents for sdist + git URL).

- [ ] **Step 3: Add a failing test**

In `src/lock/parser.rs::tests` (or wherever existing parser tests live), add:

```rust
#[test]
fn url_parse_failures_use_bad_url_variant() {
    let lockfile_toml = r#"
version = 1
revision = 1
requires-python = ">=3.11"

[[package]]
name = "broken-pkg"
version = "1.0.0"

[[package.wheels]]
url = "::not-a-url::"
hash = "sha256:abc"
"#;
    let err = parse_lockfile(lockfile_toml).expect_err("malformed URL should error");
    match err {
        LockfileError::BadUrl { package, field, url, .. } => {
            assert_eq!(package, "broken-pkg");
            assert_eq!(field, "wheel");
            assert_eq!(url, "::not-a-url::");
        }
        other => panic!("expected BadUrl, got {:?}", other),
    }
}
```

Adapt `parse_lockfile` to whatever the actual entrypoint is.

- [ ] **Step 4: Run — verify fails**

```bash
cargo test --lib lock::parser::tests::url_parse_failures_use_bad_url_variant 2>&1 | tail -10
```

Expected: FAIL — `BadUrl` doesn't exist; current code returns `BadVersion`.

- [ ] **Step 5: Add the `BadUrl` variant**

In the `LockfileError` definition file:

```rust
#[derive(Debug, thiserror::Error)]
pub enum LockfileError {
    // ... existing variants ...

    #[error("bad URL for package '{package}' field '{field}': {url} ({reason})")]
    BadUrl {
        package: String,
        field: &'static str,
        url: String,
        reason: String,
    },
}
```

- [ ] **Step 6: Migrate the sdist URL parser**

In `src/lock/parser.rs`, find the block where sdist URL is parsed (probably `Url::parse(&sdist.url)` with an `.map_err(|e| LockfileError::BadVersion { ... })`). Change:

```rust
sdist_url = Url::parse(&raw.url).map_err(|e| LockfileError::BadUrl {
    package: pkg_name.to_string(),
    field: "sdist",
    url: raw.url.clone(),
    reason: e.to_string(),
})?;
```

- [ ] **Step 7: Migrate the wheel URL parser**

Same pattern, but for the wheel URL parsing in the package's `[[package.wheels]]` loop:

```rust
let wheel_url = Url::parse(&raw_wheel.url).map_err(|e| LockfileError::BadUrl {
    package: pkg_name.to_string(),
    field: "wheel",
    url: raw_wheel.url.clone(),
    reason: e.to_string(),
})?;
```

- [ ] **Step 8: Migrate the git source URL parser**

```rust
let git_url = Url::parse(&raw_git.url).map_err(|e| LockfileError::BadUrl {
    package: pkg_name.to_string(),
    field: "git",
    url: raw_git.url.clone(),
    reason: e.to_string(),
})?;
```

- [ ] **Step 9: Run new test + existing tests**

```bash
cargo test --lib lock::parser::tests::url_parse_failures_use_bad_url_variant 2>&1 | tail -5
cargo test --lib lock::parser 2>&1 | tail -10
```

Expected: new test passes, existing tests pass. If existing tests assert error messages containing "BadVersion" or "wheel URL: ...", update them to match the new `BadUrl` display.

- [ ] **Step 10: Run full suite**

```bash
cargo test 2>&1 | tail -5
cargo clippy --all-targets --locked -- -D warnings 2>&1 | tail -5
```

Expected: all tests pass; clippy clean.

- [ ] **Step 11: Commit**

```bash
git add src/error.rs src/lock/parser.rs
git commit -m "refactor(s4): LockfileError::BadUrl for URL parse failures

Adds a dedicated BadUrl variant carrying package, field
('sdist'/'wheel'/'git'), url, and reason. Migrates the three sites
in src/lock/parser.rs that previously stuffed URL parse errors into
BadVersion. Clearer error messages; addresses TECH_DEBT item from
S1 review."
```

---

### Task 22: Convert `strongconnect` in `src/lock/graph.rs` to iterative

**Files:**
- Modify: `src/lock/graph.rs`

- [ ] **Step 1: Read the current `strongconnect`**

```bash
grep -n -B2 -A40 "fn strongconnect" src/lock/graph.rs
```

Note the closure-based recursive structure: `strongconnect` calls itself on each unvisited neighbor.

- [ ] **Step 2: Write a deep-chain stress test**

In `src/lock/graph.rs::tests` (or wherever):

```rust
#[test]
fn detect_cycles_handles_deep_linear_chain_without_stack_overflow() {
    // Build a 5000-node linear chain: pkg_0 -> pkg_1 -> ... -> pkg_4999.
    // Recursive strongconnect would overflow at ~1000 nodes on a small stack.
    use crate::lock::types::{DepEdge, FirstPartyKind, Lockfile, Package, Source};
    use pep440_rs::Version;
    use pep508_rs::PackageName;
    use std::str::FromStr;
    use url::Url;

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
                Source::FirstParty { kind: FirstPartyKind::Virtual, path: ".".into() }
            } else {
                Source::Registry { url: Url::parse("https://pypi.org/simple").unwrap() }
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
```

- [ ] **Step 3: Run — expect either pass (recursive happens to survive) or stack overflow**

```bash
cargo test --lib lock::graph::tests::detect_cycles_handles_deep_linear_chain_without_stack_overflow 2>&1 | tail -10
```

If the recursive version handles 5000 nodes (likely; default Rust stack is 8MB, each frame is small), this test passes immediately. That's fine — the test still guards against future regressions when the algorithm changes.

If it stack-overflows: even better motivation for the iterative rewrite. The next steps still apply.

- [ ] **Step 4: Rewrite `strongconnect` iteratively**

This is a known transformation. The recursive Tarjan uses a closure that:
1. Pushes `v` onto a stack S.
2. Initializes `v.index`, `v.lowlink`.
3. For each successor `w`: if `w.index` undefined, recurse on `w` then update `v.lowlink`; else if `w` is on stack, update `v.lowlink`.
4. After visiting all successors, if `v.lowlink == v.index`, pop an SCC.

The iterative version maintains an explicit work stack of `Frame { node, neighbors: NeighborsIter, lowlink }`. Each iteration either:
- (entering a node) pushes the frame; initializes index/lowlink; pushes node onto SCC stack S.
- (visiting a neighbor) checks if it's been seen; if not, pushes a new frame for it (suspending the current one); if seen and on-stack, updates lowlink.
- (leaving a node) if lowlink == index, pops the SCC; updates parent's lowlink from this node's lowlink.

Reference algorithm: https://en.wikipedia.org/wiki/Tarjan%27s_strongly_connected_components_algorithm has an iterative pseudocode.

Concrete Rust shape (adapt to your `Graph` types):

```rust
fn strongconnect_iterative(graph: &Graph) -> Vec<Vec<NodeId>> {
    let mut index_counter: u32 = 0;
    let mut indices: HashMap<NodeId, u32> = HashMap::new();
    let mut lowlinks: HashMap<NodeId, u32> = HashMap::new();
    let mut on_stack: HashSet<NodeId> = HashSet::new();
    let mut scc_stack: Vec<NodeId> = Vec::new();
    let mut sccs: Vec<Vec<NodeId>> = Vec::new();

    enum Frame<'a> {
        Enter(NodeId),
        Visit {
            node: NodeId,
            neighbors: std::vec::IntoIter<NodeId>,
        },
    }
    let mut work: Vec<Frame> = Vec::new();

    for start in graph.nodes() {
        if indices.contains_key(&start) {
            continue;
        }
        work.push(Frame::Enter(start));

        while let Some(frame) = work.pop() {
            match frame {
                Frame::Enter(node) => {
                    indices.insert(node, index_counter);
                    lowlinks.insert(node, index_counter);
                    index_counter += 1;
                    scc_stack.push(node);
                    on_stack.insert(node);
                    let neighbors: Vec<NodeId> = graph.neighbors(node).collect();
                    work.push(Frame::Visit { node, neighbors: neighbors.into_iter() });
                }
                Frame::Visit { node, mut neighbors } => {
                    if let Some(next) = neighbors.next() {
                        work.push(Frame::Visit { node, neighbors });
                        if !indices.contains_key(&next) {
                            work.push(Frame::Enter(next));
                        } else if on_stack.contains(&next) {
                            let v_lowlink = *lowlinks.get(&node).unwrap();
                            let w_index = *indices.get(&next).unwrap();
                            lowlinks.insert(node, v_lowlink.min(w_index));
                        }
                    } else {
                        // All neighbors visited — close out this node.
                        if lowlinks[&node] == indices[&node] {
                            let mut scc = Vec::new();
                            loop {
                                let w = scc_stack.pop().unwrap();
                                on_stack.remove(&w);
                                scc.push(w);
                                if w == node {
                                    break;
                                }
                            }
                            sccs.push(scc);
                        }
                        // Propagate lowlink to the parent (if any).
                        // The parent's Visit frame, if it exists, is just above
                        // in the work stack. We push a sentinel to update its lowlink.
                        // ... see note below.
                    }
                }
            }
        }
    }
    sccs
}
```

The parent-lowlink update on return is the tricky part of iterative Tarjan. One clean approach: store `lowlinks` as a HashMap and have the parent re-read it lazily when its `Visit` frame resumes. Modify the "All neighbors visited" branch to update the parent's lowlink before resuming. Alternative: a tree-like recursion stack where each Visit frame, when resuming, knows to pull the previously-completed child's lowlink.

**Pragma:** if this gets gnarly, an even simpler iterative form uses a separate "callers" stack that mirrors the work stack. Look up "iterative Tarjan SCC Rust" for known-good reference implementations and adapt.

Implementer judgment: spend up to 90 minutes; if iterative Tarjan is taking longer, **commit what you have so far as a draft** and continue. The test (5000-node chain) is the gate. If it passes with whatever you ended up with, ship it.

- [ ] **Step 5: Verify all graph tests still pass**

```bash
cargo test --lib lock::graph 2>&1 | tail -10
```

Expected: all 13 (or whatever) graph tests pass, including the new deep-chain stress test.

- [ ] **Step 6: Run full suite + clippy**

```bash
cargo test 2>&1 | tail -5
cargo clippy --all-targets --locked -- -D warnings 2>&1 | tail -5
```

- [ ] **Step 7: Commit**

```bash
git add src/lock/graph.rs
git commit -m "refactor(s4): iterative Tarjan SCC in lock::graph

Converts strongconnect from recursive (closure-based) to iterative
using an explicit work stack. Algorithm output unchanged; SCC tests
pass without modification. New stress test (5000-node linear chain)
guards against future regression to recursive form."
```

---

## Phase 7 — Docs

### Task 23: Edit main design spec §1 — remove free-threaded Python non-goal

**Files:**
- Modify: `docs/superpowers/specs/2026-05-20-muntjac-design.md`

- [ ] **Step 1: Locate the non-goal**

```bash
grep -n "Free-threaded" docs/superpowers/specs/2026-05-20-muntjac-design.md
```

Expected: one match around line 25, of the form `- **Free-threaded Python (PEP 703).** ...`.

- [ ] **Step 2: Delete the line**

Open the file at the matched line; delete the entire `- **Free-threaded Python...**` bullet (one line, including the leading `-`). Leave the surrounding bullets intact.

- [ ] **Step 3: Verify the edit**

```bash
grep -n "Free-threaded\|PEP 703" docs/superpowers/specs/2026-05-20-muntjac-design.md
```

Expected: no matches.

- [ ] **Step 4: Commit**

```bash
git add docs/superpowers/specs/2026-05-20-muntjac-design.md
git commit -m "docs(s4): drop free-threaded Python (PEP 703) non-goal

S4 lifts the v1 non-goal so the project no longer documents
free-threading as out of scope. Implementation is still deferred —
the TECH_DEBT entry is retargeted to S5 or later in a follow-up
commit."
```

---

### Task 24: Update `TECH_DEBT.md` — move resolved items, retarget cp313t

**Files:**
- Modify: `docs/superpowers/TECH_DEBT.md`

- [ ] **Step 1: Locate the three S4-affected items**

```bash
grep -n -B1 "BadVersion error\|Tarjan SCC\|cp313t" docs/superpowers/TECH_DEBT.md
```

You should see:
- "BadVersion error variant overloaded for URL parse failures" (under S1 review section)
- "Tarjan SCC is recursive..." (under S1 self-reports)
- "cp313t free-threaded ABI not first-class" (under S2 review)

- [ ] **Step 2: Move BadVersion → BadUrl to Resolved**

Cut the BadVersion section. Append to `## Resolved`:

```markdown
### `BadVersion` error variant overloaded for URL parse failures
- **Resolved:** S4, commit `<sha-of-Task-21>`
- **Summary:** Added `LockfileError::BadUrl { package, field: &'static str, url, reason }`. `src/lock/parser.rs` migrated all three URL parse sites (sdist, wheel, git) off `BadVersion`. New unit test asserts the variant on malformed wheel URLs.
```

Replace `<sha-of-Task-21>` with the actual commit SHA — find via `git log --oneline -10`.

- [ ] **Step 3: Move Tarjan SCC to Resolved**

Cut the Tarjan SCC section. Append:

```markdown
### Tarjan SCC is recursive — could stack-overflow on adversarial input
- **Resolved:** S4, commit `<sha-of-Task-22>`
- **Summary:** Converted `src/lock/graph.rs::strongconnect` to iterative form using an explicit `Vec<Frame>` work stack. Algorithm output unchanged; existing SCC tests pass without modification. New stress test asserts a 5000-node linear chain processes without stack overflow.
```

- [ ] **Step 4: Retarget cp313t**

Find the cp313t section. Change the `**Target:**` line from `S4+ once Python 3.13 free-threading stabilizes (PEP 703 finalized).` to:

```markdown
- **Target:** S5 or later. The free-threaded Python non-goal in the main design spec was lifted in S4, so this item is no longer constrained by the project's posture. Implementation deferred until PEP 703 has more real-world signal or a user requests it.
```

- [ ] **Step 5: Commit**

```bash
git add docs/superpowers/TECH_DEBT.md
git commit -m "docs(s4): TECH_DEBT — move BadUrl + Tarjan to Resolved; retarget cp313t

BadVersion->BadUrl and recursive->iterative Tarjan both shipped in
S4. cp313t (free-threaded ABI) retargeted from 'S4+' to 'S5 or later'
since S4 lifted the free-threaded non-goal — implementation can land
whenever someone has a real use case."
```

---

### Task 25: Update fixtures README

**Files:**
- Modify: `tests/fixtures/buck/README.md`

- [ ] **Step 1: Read current README**

```bash
cat tests/fixtures/buck/README.md
```

- [ ] **Step 2: Append multi-cell convention paragraph**

At the end of the file, add:

```markdown
## Multi-cell fixtures (`02-*` and later)

Fixtures from `02-numpy-pandas` onward exercise a (N platforms × M pythons) cell matrix. The `uv.lock` is frozen alongside the `expected/` goldens; regeneration touches both in one commit. To regenerate a fixture's goldens:

```bash
cd tests/fixtures/buck/<fixture>
rm -rf third-party/python/
cargo run --release --manifest-path ../../../../Cargo.toml -- buckify
cp third-party/python/BUCK expected/BUCK
cp third-party/python/muntjac.bzl expected/muntjac.bzl
cp third-party/python/PACKAGE expected/PACKAGE
cp third-party/python/config/BUCK expected/config/BUCK
```

For fixtures with a `tests/smoke/` directory (e.g. `02-numpy-pandas`), the CI workflow additionally runs `buck2 run //tests/smoke:numpy_demo` from the fixture root. The fixture is a buck2 cell — see its `.buckconfig` and the `prelude/` git submodule.
```

- [ ] **Step 3: Commit**

```bash
git add tests/fixtures/buck/README.md
git commit -m "docs(s4): document multi-cell fixture convention in README

Adds a section explaining how to regenerate multi-cell fixture
goldens (02-numpy-pandas onward) and noting which fixtures double
as buck2 cells with tests/smoke targets."
```

---

### Task 26: Delete the spike scratch notes + mark S4 ✅ in roadmap

**Files:**
- Delete: `docs/superpowers/scratch-s4-spike.md`
- Modify: `docs/superpowers/specs/2026-05-20-muntjac-roadmap.md`

- [ ] **Step 1: Delete the spike scratch notes**

```bash
rm docs/superpowers/scratch-s4-spike.md
```

The notes served their purpose driving Phase 3 emitter rendering. Their content is now expressed in the emitter code + `tests/fixtures/buck/02-numpy-pandas/expected/PACKAGE` (the validated shape). Don't leave the scratch file around — it'd become stale.

- [ ] **Step 2: Read current roadmap S4 status**

```bash
grep -n -A2 "S4" docs/superpowers/specs/2026-05-20-muntjac-roadmap.md | head -10
```

Expected: a table row like `| S4 | (not yet written) | (not yet written) | ⬜ next |`.

- [ ] **Step 3: Update S4 row**

Edit the roadmap's "Specs index" table. Replace the S4 row with:

```markdown
| S4 | [2026-05-21-muntjac-s4-multiplatform-design.md](./2026-05-21-muntjac-s4-multiplatform-design.md) | [2026-05-21-muntjac-s4-multiplatform.md](../plans/2026-05-21-muntjac-s4-multiplatform.md) | ✅ shipped (tag `s4-complete`, <N commits>, <N tests>) |
```

The `<N commits>` and `<N tests>` fill in at end-of-stage. Use:

```bash
# commits since branching point (S3 complete tag)
git log s3-complete..HEAD --oneline | wc -l
# test count
cargo test 2>&1 | grep "test result" | tail -1
```

Also update the S5 row from `⬜ blocked on S4` to `⬜ next`.

- [ ] **Step 4: Commit**

```bash
git add docs/superpowers/specs/2026-05-20-muntjac-roadmap.md
git rm docs/superpowers/scratch-s4-spike.md
git commit -m "docs(s4): mark S4 ✅ shipped in roadmap; delete spike scratch notes

<N commits> on this stage; full suite at <N> tests. The 02-numpy-pandas
fixture builds + runs under buck2 on all three CI runners. Closes
roadmap stage S4."
```

- [ ] **Step 5: Tag the stage**

```bash
git tag -a s4-complete -m "S4: multi-platform + multi-python BUCK ('numpy day one')"
git push --tags
```

---

## Self-review checklist

After all tasks land green:

- [ ] All spec §1 in-scope items are implemented (emitter, PACKAGE, fixtures, CI, tech debt).
- [ ] All spec §1 out-of-scope items are NOT implemented (native sdists, fixups, vendor, cp313t code, muntjac_python_binary macro).
- [ ] `cargo test` is green on the local machine (~156 tests).
- [ ] `cargo clippy --all-targets --locked -- -D warnings` is clean.
- [ ] CI is green on all three matrix runners with the buck2 smoke step.
- [ ] `tests/fixtures/buck/02-numpy-pandas/expected/` matches what `cargo run -- buckify` produces (determinism check by re-running locally).
- [ ] `docs/superpowers/specs/2026-05-20-muntjac-design.md` §1 no longer lists free-threaded Python as a non-goal.
- [ ] `docs/superpowers/TECH_DEBT.md` has BadUrl + iterative Tarjan in `## Resolved` and cp313t retargeted.
- [ ] Roadmap shows S4 ✅ shipped, S5 ⬜ next.
- [ ] `scratch-s4-spike.md` is deleted.
- [ ] Stage tagged `s4-complete`.

Spec coverage gaps to flag here: none anticipated. If a gap surfaces, file a new follow-up in `TECH_DEBT.md` rather than expanding S4's scope mid-stage.
