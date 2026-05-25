# muntjac

> Translate `uv.lock` into [Buck2](https://buck2.build/) build rules.

[![CI](https://github.com/rsJames-ttrpg/muntjac/actions/workflows/ci.yml/badge.svg)](https://github.com/rsJames-ttrpg/muntjac/actions/workflows/ci.yml)
[![Crates.io](https://img.shields.io/crates/v/muntjac.svg)](https://crates.io/crates/muntjac)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

## What is muntjac?

**Problem.** `uv` is the modern Python dependency resolver — fast, correct, and what most Python teams are migrating to. But `uv` doesn't speak Buck. If your monorepo uses Buck2, you've had to either hand-write `pypi_package` rules per dependency or use Reindeer (which is Cargo-only).

**Solution.** muntjac reads `uv.lock` plus a small `muntjac.toml` config and emits `BUCK`, `muntjac.bzl`, `wiring.bzl`, and `config/BUCK`. PEP 503 normalization, marker evaluation per platform, PEP 517 sdist prebake, and native-extension fixups are all handled.

**Moat.** Community fixup registry at [`github.com/rsJames-ttrpg/muntjac-fixups`](https://github.com/rsJames-ttrpg/muntjac-fixups). Native deps (libjpeg, openssl, libzmq, etc.) are notoriously fiddly — the registry means you don't write those incantations yourself.

**Developer experience.** muntjac flattens Buck2's learning curve for Python teams. Keep using `uv add` / `uv lock` / `uv sync` for day-to-day dependency work — muntjac re-derives Buck rules from `uv.lock` on demand. The mental model stays *edit pyproject.toml → re-buckify*; you never hand-edit `pypi_package` rules, never look up wheel filenames, never debug marker evaluation by hand. uv-native ergonomics in, Buck-native targets out.

## Quickstart

Prereqs: a working Buck2 project (with `prelude/`, `toolchains/`, `.buckconfig`, `PACKAGE`). If you don't have one, see [Setting up Buck2](#setting-up-buck2) below. Then:

```sh
cargo install muntjac
cd my-py-project          # contains pyproject.toml
muntjac init              # writes muntjac.toml + third-party/python/ skeleton
uv add numpy
muntjac vendor            # prebake pure-Python sdists into wheels
muntjac buckify           # emit BUCK + muntjac.bzl + wiring.bzl + config/BUCK
buck2 run //app:main
```

That's it. Re-run `muntjac buckify` whenever `uv.lock` changes.

## Setting up Buck2

If you're new to Buck2, the easiest starter is to copy the `prelude/`, `toolchains/`, `.buckconfig`, and `PACKAGE` files from [muntjac's fixture 02](https://github.com/rsJames-ttrpg/muntjac/tree/main/tests/fixtures/buck/02-numpy-pandas) into your project root. They wire up the [facebook/buck2-prelude](https://github.com/facebook/buck2-prelude) and a Python toolchain rooted at `python3.12`.

If you have an existing Buck2 setup, muntjac just needs `[repositories] prelude = ...` in `.buckconfig` and a working `system_python_toolchain` named `//toolchains:python`.

## Configuration

`muntjac init` writes a starter `muntjac.toml`. The interesting fields:

```toml
manifest_path   = "pyproject.toml"
third_party_dir = "third-party/python"
python_versions = ["3.11", "3.12"]

[platforms.linux-x86_64-gnu]
target    = "x86_64-unknown-linux-gnu"
manylinux = "2_17"

[platforms.macos-arm64]
target    = "aarch64-apple-darwin"
macos_min = "11.0"

# Optional — include PEP 735 dependency groups:
# [lockfile]
# include_groups = ["test"]

[fixups]
registry              = "none"      # or "github.com/rsJames-ttrpg/muntjac-fixups"
allow_local_overrides = true

[buck]
file_name = "BUCK"
vendor    = false
```

Full schema reference: [design spec §3 — configuration](docs/superpowers/specs/2026-05-20-muntjac-design.md).

## Multi-tree (incompatible dependency universes)

When parts of your monorepo can't share one resolution — say a legacy service needs `numpy<2` and a new one needs `numpy>=2` — declare one `[tree.<name>]` block per universe. `[platforms]` and `[fixups]` stay shared across all trees:

```toml
[platforms]
macos-arm64 = { target = "aarch64-apple-darwin", macos_min = "11.0" }

[tree.modern]
manifest_path   = "pyproject.toml"
third_party_dir = "third-party/python/modern"
python_versions = ["3.12"]

[tree.legacy]
manifest_path   = "legacy/pyproject.toml"
third_party_dir = "third-party/python/legacy"
python_versions = ["3.12"]
```

Each tree is an island — the same package at conflicting versions coexists via distinct Buck target paths (`//third-party/python/modern:numpy` vs `//third-party/python/legacy:numpy`). `muntjac buckify` and `muntjac vendor` process all trees by default; `--tree <name>` scopes to one. `muntjac fixups show <pkg>` prints a per-tree block. First-party rules pick a universe by which target path they depend on.

The Buck cfg machinery (`config/` + `wiring.bzl`) is emitted once at a shared `cfg_dir` — by default the longest common ancestor of the trees' `third_party_dir`s (here `third-party/python`), overridable via `[buck] cfg_dir`.

## Community fixups

Most Python packages work out of the box. Some — packages with C extensions linking libjpeg, openssl, libzmq — need a *fixup* that wires the wheel up to the right `//third-party/c:*` targets.

Opt in via:

```toml
[fixups]
registry = "github.com/rsJames-ttrpg/muntjac-fixups"
```

Then `muntjac fixups update` fetches the latest pinned SHA. Layered model: community fixups are applied first, then any in-tree `third-party/python/fixups/<pkg>.toml` overrides win on scalar fields and extend on lists. A local fixup can set `replace_community = true` to bypass the community entry entirely.

See [muntjac-fixups README](https://github.com/rsJames-ttrpg/muntjac-fixups#readme) for the seed package list (pillow, cryptography, lxml, pyzmq, psycopg2-binary) and [CONTRIBUTING](https://github.com/rsJames-ttrpg/muntjac-fixups/blob/main/CONTRIBUTING.md) for how to add one.

## Status

v0.2.0. Linux x86_64 + macOS arm64 are the credible-launch platforms — numpy, pandas, fastapi, requests, and ruff are confirmed working end-to-end. Linux arm64 is supported and tested in CI. Multi-tree (incompatible dependency universes) is supported as of v0.2.0. Windows + Intel macOS work via `cargo install muntjac` (compiles from source); prebuilt binaries are planned post-launch.

See [roadmap](docs/superpowers/specs/2026-05-20-muntjac-roadmap.md) for upcoming plans (vendor mode, audit/unused).

## Contributing

Issues + PRs welcome. See [CONTRIBUTING.md](CONTRIBUTING.md) once it lands; in the meantime, see [`docs/superpowers/specs/`](docs/superpowers/specs/) for the design specs and [`docs/superpowers/TECH_DEBT.md`](docs/superpowers/TECH_DEBT.md) for the open ledger.

## License

MIT — see [LICENSE](LICENSE).
