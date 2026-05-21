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
