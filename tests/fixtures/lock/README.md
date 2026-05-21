# Lockfile fixtures

Each directory contains:
- `muntjac.toml` — config
- `uv.lock` — **frozen artifact** (do not regenerate without also regenerating goldens)
- `pyproject.toml` (sometimes) — used to regenerate `uv.lock` if needed
- `expected-*.json` / `expected-*.txt` — golden outputs

## Regenerating a fixture

If you need to update a fixture (e.g. because the on-disk `uv.lock` needs
to reflect a newer upstream version), regenerate **both** the lockfile and the
matching golden(s) in a single commit. Reviewers should diff both. If you
regenerate only the lockfile, the test will fail with a phantom mismatch.

## Why the lockfiles are committed

`uv lock` resolves against live PyPI. If our tests called `uv lock`, they
would break whenever PyPI changes (yanked releases, new versions). Committing
the resolved lockfile freezes the test inputs.
