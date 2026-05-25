# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] — 2026-05-25

### Added

- First public release.
- `muntjac init` — scaffold `muntjac.toml` and `third-party/python/` skeleton.
- `muntjac config check` — validate `muntjac.toml`.
- `muntjac vendor` — prebake pure-Python sdists into wheels.
- `muntjac buckify` — emit `BUCK`, `muntjac.bzl`, `wiring.bzl`, `config/BUCK` from `uv.lock`.
- `muntjac fixups show` — print the resolved fixup for a package.
- `muntjac fixups update [--rev <rev>]` — fetch the community fixup registry.
- Layered fixups: community (Git-based registry) + local (in-tree) with documented merge rules.
- Credible-launch surface: numpy, pandas, fastapi, requests, ruff on Linux x86_64, Linux arm64, and macOS arm64.
- Seed community fixup registry at github.com/rsJames-ttrpg/muntjac-fixups (5 packages: pillow, cryptography, lxml, pyzmq, psycopg2-binary).

### Known limitations

- `muntjac unused`, `muntjac audit` not yet implemented (planned for v0.2+).
- Windows + Intel macOS prebuilt binaries not in this release; install via `cargo install muntjac` works on those platforms.
- See `docs/superpowers/TECH_DEBT.md` for the full ledger.

[Unreleased]: https://github.com/rsJames-ttrpg/muntjac/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/rsJames-ttrpg/muntjac/releases/tag/v0.1.0
