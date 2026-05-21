# S4 spike findings (consumed by Phase 3 emitter rendering; deleted at end of S4)

Validates the cfg-modifier shape against the actual open-source buck2 +
buck2-prelude before Phase 3 (T9-T13) commits an emitter that renders this
shape. The hand-written files used during the spike all live under
`tests/fixtures/buck/02-numpy-pandas/` and were iterated until
`buck2 run //tests/smoke:numpy_demo` printed `[0. 0. 0.]` from a clean state.

## buck2 install

- Version: `2026-05-18`
- Git SHA: `3f054b09fb3ddf6e96c8c38f1f21e9420b4215f0`
- Path on spike host: `/home/jackm/.local/bin/buck2`
- URL: `https://github.com/facebook/buck2/releases/download/2026-05-18/buck2-x86_64-unknown-linux-gnu.zst`

## prelude

- Submodule URL: `https://github.com/facebook/buck2-prelude.git`
- Pinned SHA: `b4e55417b4edf582be8fb20f24e1afc5866987ce`
- Mounted at: `tests/fixtures/buck/02-numpy-pandas/prelude/`
- Cell name: `prelude` (with `config` / `ovr_config` aliased to `prelude` in `.buckconfig`)

## Validated load() paths

| Symbol                        | Validated load path                                                |
|-------------------------------|--------------------------------------------------------------------|
| `python_binary` rule          | NATIVE - no load needed                                            |
| `prebuilt_python_library` rule| NATIVE - no load needed (use `native.prebuilt_python_library` in `.bzl`) |
| `http_file` rule              | NATIVE - no load needed (use `native.http_file` in `.bzl`)         |
| `set_cfg_modifiers`           | `@prelude//cfg/modifier:set_cfg_modifiers.bzl`                     |
| `set_cfg_constructor`         | NATIVE built-in (no load) - called once from root `PACKAGE`        |
| `cfg_constructor_*` impls     | `@prelude//cfg/modifier:cfg_constructor.bzl`                       |
| `MODIFIER_METADATA_KEY`       | `@prelude//cfg/modifier:common.bzl`                                |
| `system_python_toolchain`     | `@prelude//toolchains:python.bzl`                                  |
| `system_cxx_toolchain`        | `@prelude//toolchains:cxx.bzl`                                     |
| `expect`                      | `@prelude//utils:expect.bzl` - NOT used in the spike; replaced with `fail("...")` after a key/list comparison since `expect` is awkward in a deterministic generator |

The spec draft assumed `python_binary`, `prebuilt_python_library`, and
`http_file` were Starlark-loadable from their `.bzl` files. In this prelude
they are NOT - only `_impl` functions are exported from those `.bzl` files;
the rules themselves are registered as native rules in `prelude/rules_impl.bzl`.
Macro authors call them via the bare name in `BUCK` files and via
`native.<rule>` from `.bzl` files.

## Validated cell prefix for OS/CPU constraints

- OS constraint setting: `prelude//os/constraints:os`
- OS constraint values: `prelude//os/constraints:{linux,macos,windows,android,...}` (from `prelude/os/constraints/BUCK`)
- CPU constraint setting: `prelude//cpu/constraints:cpu`
- CPU constraint values: `prelude//cpu/constraints:{x86_64,arm64,arm32,...}` (from `prelude/cpu/constraints/BUCK`)

The `.buckconfig` aliases `config = prelude` and `ovr_config = prelude`, so
`config//os/constraints:linux` and `ovr_config//os/constraints:linux` also
work. The Phase-3 emitter should pick ONE canonical form and stick to it -
the spike used `prelude//` directly because the underlying targets live in
the `prelude` cell and the alias is only convenience.

Important constraint targets the emitter must reference:

- `prelude//os/constraints:linux`
- `prelude//os/constraints:macos`
- `prelude//cpu/constraints:x86_64`
- `prelude//cpu/constraints:arm64`

`musllinux` does not have a first-class OS constraint in this prelude;
muntjac's cell axis distinguishes glibc vs musl, but the host-axis modifier
only branches on OS+CPU. The Phase-3 emitter cannot derive musl-vs-glibc
from `host_info()` alone; for stage 03 the user is expected to either pick
a cell explicitly via `--modifier` or rely on a separate config_setting.
This matches the spec's existing musllinux story.

## Validated `set_cfg_modifiers` shape (host-axis wiring)

The Starlark `ModifiersMatch` type is a `dict[str, typing.Any]` with a
required `"_type": "ModifiersMatch"` discriminator key (see
`prelude/cfg/modifier/types.bzl::is_modifiers_match`). Targets in keys must
be FULLY QUALIFIED (`cell//path:name`) - bare `//path:name` is rejected by
`verify_normalized_target`. Nested `ModifiersMatch` dicts are supported.

```python
load("@prelude//cfg/modifier:set_cfg_modifiers.bzl", "set_cfg_modifiers")

set_cfg_modifiers(
    cfg_modifiers = [
        {
            "_type": "ModifiersMatch",
            "prelude//os/constraints:linux": {
                "_type": "ModifiersMatch",
                "prelude//cpu/constraints:x86_64": "root//third-party/python/config:linux-x86_64-gnu",
                "prelude//cpu/constraints:arm64":  "root//third-party/python/config:linux-aarch64-gnu",
            },
            "prelude//os/constraints:macos": {
                "_type": "ModifiersMatch",
                "prelude//cpu/constraints:arm64":  "root//third-party/python/config:macos-arm64",
            },
        },
    ],
)
```

### Where the `set_cfg_modifiers` call MUST live (critical finding)

The spec drafted putting this in `third-party/python/PACKAGE`. **THAT DOES
NOT WORK.** PACKAGE-level modifiers attach to targets DEFINED under that
PACKAGE - they do NOT propagate to a dep's configuration when the dep is
resolved from a consumer in a sibling/unrelated package.

In the spike, `numpy_demo` in `tests/smoke/` depends on
`//third-party/python:numpy`. With `set_cfg_modifiers` only under
`third-party/python/PACKAGE`, `numpy_demo` is configured with `cfg:<empty>`
(plus its per-target `modifiers` for `py312`), and the platform constraint
(`linux-x86_64-gnu`) is never set on its config. The dep alias' `select()`
then resolves under `numpy_demo`'s cfg and finds no key.

The host-axis `set_cfg_modifiers` MUST live in the ROOT `PACKAGE` (or in
every consumer's PACKAGE) so the platform constraint is present on every
target's configuration before deps are resolved.

**Implication for Phase 3 (T11-T13):** the emitter must EITHER

1. write the host-axis `set_cfg_modifiers` into a root-level `PACKAGE` (one
   that lives alongside `.buckconfig` and is part of the user's project
   skeleton, not `third-party/python/PACKAGE`), or
2. document that the user must add the host-axis call to their root
   `PACKAGE` and only emit the python-version modifiers per-binary.

Option (1) is simpler and matches the spec's "auto-wired" intent. The
emitted file is then `<project-root>/PACKAGE` rather than the gitignored
`third-party/python/PACKAGE`. The spike commits a hand-written root
`PACKAGE` because the smoke test cannot run without it.

### `set_cfg_constructor` registration (critical finding)

The open-source prelude does NOT call `set_cfg_constructor` itself. Without
this call, `set_cfg_modifiers` is silently a no-op and per-target
`modifiers = [...]` attrs are ignored. The PROJECT (not the emitter) is
responsible for registering it once, in the root `PACKAGE`:

```python
load(
    "@prelude//cfg/modifier:cfg_constructor.bzl",
    "cfg_constructor_post_constraint_analysis",
    "cfg_constructor_pre_constraint_analysis",
)
load("@prelude//cfg/modifier:common.bzl", "MODIFIER_METADATA_KEY")

set_cfg_constructor(
    stage0 = cfg_constructor_pre_constraint_analysis,
    stage1 = cfg_constructor_post_constraint_analysis,
    key = MODIFIER_METADATA_KEY,
    aliases = struct(),
    extra_data = struct(),
)
```

Phase 3 should emit this block at the top of the root `PACKAGE` alongside
the host-axis `set_cfg_modifiers`.

## Validated `muntjac.bzl`

```python
"""Hand-written stand-in for the muntjac-emitted wiring macro."""

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
            "//third-party/python/config:{}".format(cfg): ":{}-{}__{}".format(name, version, cfg)
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

Notes for the Phase-3 emitter:

- `http_file`, `prebuilt_python_library`, and `alias` are native rules - in
  a `.bzl` they must be called as `native.<rule>`.
- `http_file` requires `sha256` as a bare hex string (NO `sha256:` prefix);
  the macro strips the prefix to keep the input format aligned with the
  uv.lock convention.
- The leaf `prebuilt_python_library` target's `visibility = []` keeps
  consumers from depending on a specific cell - they go through the alias.
- `select()` keys are bare `//third-party/python/config:...` here; bare
  cell-less form works inside a `BUCK`/`bzl` file in the same cell.

## Validated `config/BUCK`

```python
constraint_setting(name = "python_version")
constraint_value(name = "py311", constraint_setting = ":python_version", visibility = ["PUBLIC"])
constraint_value(name = "py312", constraint_setting = ":python_version", visibility = ["PUBLIC"])

constraint_setting(name = "platform")
constraint_value(name = "linux-x86_64-gnu",  constraint_setting = ":platform", visibility = ["PUBLIC"])
constraint_value(name = "linux-aarch64-gnu", constraint_setting = ":platform", visibility = ["PUBLIC"])
constraint_value(name = "macos-arm64",       constraint_setting = ":platform", visibility = ["PUBLIC"])

config_setting(name = "py311-linux-aarch64-gnu", constraint_values = [":linux-aarch64-gnu", ":py311"], visibility = ["PUBLIC"])
config_setting(name = "py311-linux-x86_64-gnu",  constraint_values = [":linux-x86_64-gnu",  ":py311"], visibility = ["PUBLIC"])
config_setting(name = "py311-macos-arm64",       constraint_values = [":macos-arm64",       ":py311"], visibility = ["PUBLIC"])
config_setting(name = "py312-linux-aarch64-gnu", constraint_values = [":linux-aarch64-gnu", ":py312"], visibility = ["PUBLIC"])
config_setting(name = "py312-linux-x86_64-gnu",  constraint_values = [":linux-x86_64-gnu",  ":py312"], visibility = ["PUBLIC"])
config_setting(name = "py312-macos-arm64",       constraint_values = [":macos-arm64",       ":py312"], visibility = ["PUBLIC"])
```

Notes:

- `visibility = ["PUBLIC"]` on EVERY constraint_value/config_setting is
  required because `set_cfg_modifiers` resolves these from the root PACKAGE
  (different cell-relative path).
- `constraint_setting` does not need `visibility` (it is only referenced
  via `constraint_value(constraint_setting = ...)` from inside the same
  package).

## Python toolchain registration

```python
load("@prelude//toolchains:cxx.bzl", "system_cxx_toolchain")
load("@prelude//toolchains:python.bzl", "system_python_bootstrap_toolchain", "system_python_toolchain")

_PY312 = "/home/jackm/.local/share/uv/python/cpython-3.12.0-linux-x86_64-gnu/bin/python3.12"

system_python_bootstrap_toolchain(
    name = "python_bootstrap",
    interpreter = _PY312,
    visibility = ["PUBLIC"],
)

system_python_toolchain(
    name = "python",
    interpreter = _PY312,
    visibility = ["PUBLIC"],
)

system_cxx_toolchain(
    name = "cxx",
    visibility = ["PUBLIC"],
)
```

Findings:

- `python_binary` requires BOTH `toolchains//:python` AND
  `toolchains//:cxx` (and an implicit `cxx_no_default_deps` derived from
  `cxx`). The first attempt failed with `Unknown target 'cxx' from package
  'toolchains//'`.
- The `_python_bootstrap` toolchain is required for any wheel extraction
  (it runs `prebuilt_python_library`'s extract tool).
- The default `system_python_*_toolchain(interpreter = ...)` accepts a
  bare command name (looked up on PATH) OR an absolute path. CI must
  install `python3.12` (cp312-compatible) and either symlink it onto PATH
  or pass an absolute path. The spike used the absolute path because
  `python3.12` was not on PATH on the host.
- The Phase-3 emitter does NOT generate `toolchains/BUCK` - it is part of
  the user's project skeleton (same tier as `.buckconfig`).

## `.buckconfig` change required by the spike

The fixture's original `.buckconfig` had:

```
[project]
  ignore = .git, prelude/.git, third-party
```

This prevented buck2 from loading anything under `third-party/`. The spike
removed `third-party` from `project.ignore`:

```
[project]
  ignore = .git, prelude/.git
```

Phase 3 implication: the muntjac-emitted `.buckconfig` (if muntjac emits
one) must NOT add `third-party` to `project.ignore`, even though the
generated files there are reproducible from `uv.lock`. They are
consumed targets and must be loadable.

## Output of successful run

```
$ buck2 clean && buck2 run //tests/smoke:numpy_demo 2>&1 | tail -5
Starting new buck2 daemon...
Connected to new buck2 daemon.
Build ID: a4b1f6dd-d572-4045-b8d3-e348bb6043df
BUILD SUCCEEDED - starting your binary
[0. 0. 0.]
```

## Per-target `modifiers = [...]` attribute

The spec asked whether the per-binary `modifiers` attribute works in this
prelude version. **It does**:

```python
python_binary(
    name = "numpy_demo",
    main = "demo.py",
    modifiers = ["//third-party/python/config:py312"],
    deps = ["//third-party/python:numpy"],
)
```

`buck2 audit configurations` shows the resulting cfg includes
`root//third-party/python/config:py312` propagated to deps. No fallback
to `buck2 run --modifier ...` was needed. The `modifiers` attribute is a
buck2-core attribute (not declared per-rule in the prelude), so it works
on `python_binary` as long as the cfg_constructor is registered.

## Notes / surprises

1. **`set_cfg_constructor` is NOT in the open-source prelude.** Without
   the explicit registration in the root PACKAGE, set_cfg_modifiers AND
   the `modifiers` attribute are both silently no-ops. This was the
   subtlest finding: `buck2 audit configurations` showed `cfg:<empty>` in
   all branches even after writing PACKAGE+modifiers correctly. Only after
   registering `set_cfg_constructor` did the modifiers begin to apply.

2. **PACKAGE modifiers don't cross package boundaries through deps.** A
   `set_cfg_modifiers` in `third-party/python/PACKAGE` does NOT affect the
   configuration of `//third-party/python:numpy` when it is configured as
   a dep of a target in `tests/smoke/`. The dep inherits the consumer's
   cfg, and PACKAGE modifiers attach to the DEFINING package's targets,
   not to deps resolved into the package. Host-axis MUST live at the root.

3. **`ModifiersMatch` requires `"_type": "ModifiersMatch"`.** The dict
   literal `{"prelude//os/constraints:linux": {...}}` is rejected because
   `is_modifiers_match` looks for the `_type` discriminator. This is not
   obvious from the doc string of `set_cfg_modifiers`.

4. **Constraint targets must be fully qualified.** `//third-party/...` is
   rejected; `root//third-party/...` is required. The `verify_normalized_target`
   check looks for the cell prefix.

5. **The wheel-tag-to-config mapping is straightforward.** numpy ships
   `cp{311,312}-cp{311,312}-{manylinux_2_17_aarch64,manylinux_2_17_x86_64,macosx_11_0_arm64}.whl`
   tags that map cleanly to our 6 cells. No surprises; muntjac's existing
   tag classifier should handle this without changes for stage 02.

6. **CPython version mismatch is silent until import.** If the wheel is
   cp312 but the interpreter is cp313/cp314, the build succeeds; the
   import fails at runtime with a NumPy-specific error about the C
   extension. Phase 3 should not surface this - the user is expected to
   provide a matching interpreter via the toolchain. But CI must install
   a cp312 interpreter for the smoke to pass.

7. **Native rules vs Starlark wrappers.** The spec's `load(...)` calls for
   `python_binary` / `prebuilt_python_library` / `http_file` would have
   failed - all three are native rules with no Starlark exports. The
   emitter should NOT emit these `load()` lines.

8. **No `expect(...)` used.** The `@prelude//utils:expect.bzl::expect`
   helper expects a `bool` first arg, but the spec example passed a set
   comparison that I replaced with a plain `if` + `fail(...)` for clarity.
   Either form works in the macro.
