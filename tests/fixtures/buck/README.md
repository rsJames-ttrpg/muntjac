# BUCK emitter fixtures

Each directory contains:
- `muntjac.toml` — config (platforms + python versions to exercise)
- `pyproject.toml` — input used to regenerate `uv.lock`
- `uv.lock` — **frozen artifact**
- `expected/` — golden output files (BUCK, muntjac.bzl, PACKAGE, config/BUCK)

See `tests/fixtures/lock/README.md` for the broader frozen-artifact
convention. The same rule applies: regenerating `uv.lock` requires
regenerating all goldens in the same commit.

## Regenerating goldens

If the BUCK emitter output shape changes:

```bash
cd tests/fixtures/buck/<fixture>
tmp=$(mktemp -d)
cp pyproject.toml uv.lock muntjac.toml "$tmp/"
cargo run --quiet --manifest-path=../../../../Cargo.toml -- -C "$tmp" buckify
cp "$tmp/third-party/python/BUCK"        expected/BUCK
cp "$tmp/third-party/python/muntjac.bzl" expected/muntjac.bzl
cp "$tmp/third-party/python/PACKAGE"     expected/PACKAGE
cp "$tmp/third-party/python/config/BUCK" expected/config/BUCK
```

## Multi-cell fixtures (`02-*` and later)

Fixtures from `02-numpy-pandas` onward exercise a (N platforms × M pythons) cell matrix. The `uv.lock` is frozen alongside the `expected/` goldens; regeneration touches both in one commit.

To regenerate a fixture's goldens, from the fixture root:

    rm -rf third-party/python/
    cargo run --release --manifest-path ../../../../Cargo.toml -- buckify
    cp third-party/python/BUCK expected/BUCK
    cp third-party/python/muntjac.bzl expected/muntjac.bzl
    cp third-party/python/wiring.bzl expected/wiring.bzl
    cp third-party/python/config/BUCK expected/config/BUCK

For fixtures with a `tests/smoke/` directory (e.g. `02-numpy-pandas`), the CI workflow additionally runs `buck2 run //tests/smoke:numpy_demo` from the fixture root. The fixture is a buck2 cell — see its `.buckconfig` and the `prelude/` git submodule.

`<third_party_dir>/PACKAGE` is NOT emitted by muntjac. The host-axis cfg wiring lives in `<third_party_dir>/wiring.bzl` (`MUNTJAC_HOST_MODIFIERS`); the user's root PACKAGE loads it and calls `set_cfg_modifiers` directly. See the wiring.bzl header in any generated fixture for the canonical user-side snippet.
