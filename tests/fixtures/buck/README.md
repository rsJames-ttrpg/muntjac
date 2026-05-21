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
