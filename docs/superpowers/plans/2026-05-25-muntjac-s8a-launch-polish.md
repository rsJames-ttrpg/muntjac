# S8a — muntjac repo polish & v0.1.0 launch — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Polish the muntjac repo for public consumption: README rewrite, demo CI fixture, Cargo metadata, LICENSE, CHANGELOG, release workflow (cargo publish + cross-compiled GH Release binaries), launch post stub. Tag-cut and `cargo publish` themselves are manual maintainer actions, gated on this plan landing.

**Architecture:** Pure additive — no logic changes to `src/`. Adds LICENSE, CHANGELOG, three new GitHub Actions workflows, rewrites README, polishes `Cargo.toml`, swaps one comment block in `muntjac init`. The demo CI fixture exercises the README flow end-to-end against real PyPI on all three matrix runners, by copying fixture 02's prelude/toolchains/.buckconfig scaffolding into a tmpdir and running `uv add → muntjac init → muntjac vendor → muntjac buckify → buck2 run`.

**Tech Stack:** Rust 2024 edition (no code changes), GitHub Actions YAML, Cargo metadata, MIT LICENSE text, Keep-a-Changelog format, `awk` for release-notes extraction.

**Spec:** [`docs/superpowers/specs/2026-05-25-muntjac-s8a-launch-polish-design.md`](../specs/2026-05-25-muntjac-s8a-launch-polish-design.md)

---

## File Structure

**Create:**
- `LICENSE` — MIT license text (Task 1)
- `CHANGELOG.md` — Keep-a-Changelog with [Unreleased] + [0.1.0] (Task 3)
- `.github/workflows/publish-check.yml` — dedicated `cargo publish --dry-run` gate (Task 5)
- `.github/workflows/release.yml` — tag-triggered: dry-run → publish → cross-compile → GH Release (Task 6)
- `.github/workflows/demo.yml` — 60-second README demo on all 3 matrix runners (Task 7)
- `README.md` — full rewrite, replaces the 12-line stub (Task 8)
- `docs/launch-post.md` — short/medium/long-form drafts (Task 9)

**Modify:**
- `Cargo.toml` — version bump, repository URL, add authors/keywords/categories/homepage/documentation (Task 2)
- `src/cli/init.rs` — replace the registry-URL hint comment with concrete example (Task 4)
- `tests/init.rs` — add substring assertion for the new comment text (Task 4)
- `docs/superpowers/specs/2026-05-20-muntjac-roadmap.md` — mark S8a ✅ shipped at stage close (Task 10)

**No source-code logic changes.** `muntjac vendor`, `muntjac buckify`, and `muntjac init` keep their current behavior. Only the init template's *comment text* changes.

---

## Task ordering rationale

1. **LICENSE first** — smallest, mechanical; `cargo publish --dry-run` (Task 5's workflow) prefers a real file at repo root.
2. **Cargo.toml polish** — depends on LICENSE existing; sets the version + metadata that Task 3 (CHANGELOG) and Task 6 (release.yml) reference.
3. **CHANGELOG.md** — references the version from Task 2; Task 6's `awk` extractor expects this file.
4. **`muntjac init` template update** — independent, small. Pairs with a substring-assertion update to existing init tests.
5. **publish-check.yml** — depends on Tasks 1-2 (license + metadata) being correct so the dry-run actually passes.
6. **release.yml** — depends on Tasks 1-3 + 5; lints standalone since the workflow only runs at tag-cut time.
7. **demo.yml** — independent; depends on muntjac being buildable from this checkout (it is).
8. **README.md** — last among artifact tasks so all referenced URLs / file paths exist when the README links to them.
9. **launch-post.md** — independent; near end since it cites README structure.
10. **Roadmap mark-shipped** — final stage-close commit, captures commit count + tag.

---

## Task 1: LICENSE file

**Files:**
- Create: `LICENSE`

- [ ] **Step 1: Verify the file does not already exist**

Run: `test ! -e /home/jackm/repos/muntjac/LICENSE && echo missing-as-expected`
Expected: `missing-as-expected`

- [ ] **Step 2: Write `LICENSE` at repo root**

Exact contents (standard MIT, copyright Jack Mayo, year 2026):

```
MIT License

Copyright (c) 2026 Jack Mayo

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

- [ ] **Step 3: Verify file size + first/last line**

Run:
```sh
wc -l /home/jackm/repos/muntjac/LICENSE
head -1 /home/jackm/repos/muntjac/LICENSE
tail -1 /home/jackm/repos/muntjac/LICENSE
```
Expected:
- Line count: 21 (standard MIT layout)
- First line: `MIT License`
- Last line: `SOFTWARE.`

- [ ] **Step 4: Commit**

```sh
cd /home/jackm/repos/muntjac
git add LICENSE
git commit -m "$(cat <<'EOF'
docs: add LICENSE file (MIT)

cargo publish prefers a real LICENSE file at repo root in addition to
the `license = "MIT"` field in Cargo.toml. Adding it also enables
GitHub's License-detection sidebar and satisfies cargo-deny / FOSSA
downstream checks.

Copyright (c) 2026 Jack Mayo.
EOF
)"
```

---

## Task 2: Polish Cargo.toml metadata

**Files:**
- Modify: `Cargo.toml` (lines 1-9 — the `[package]` table)

- [ ] **Step 1: Read current `Cargo.toml` to confirm exact `[package]` block**

Run: `sed -n '1,18p' /home/jackm/repos/muntjac/Cargo.toml`
Expected output (record the exact lines so the Edit tool's old_string matches):
```toml
[package]
name = "muntjac"
version = "0.1.0-dev"
edition = "2024"
rust-version = "1.85"
description = "Translate uv.lock into Buck2 build rules"
license = "MIT"
repository = "https://github.com/jackmpcollins/muntjac"
readme = "README.md"
```

- [ ] **Step 2: Replace the `[package]` block with the polished version**

Use the Edit tool. Replace the 9-line block above with this 13-line block:

```toml
[package]
name          = "muntjac"
version       = "0.1.0"
edition       = "2024"
rust-version  = "1.85"
description   = "Translate uv.lock into Buck2 build rules"
authors       = ["Jack Mayo <jackomayo@gmail.com>"]
license       = "MIT"
readme        = "README.md"
repository    = "https://github.com/rsJames-ttrpg/muntjac"
homepage      = "https://github.com/rsJames-ttrpg/muntjac"
documentation = "https://docs.rs/muntjac"
keywords      = ["buck2", "python", "uv", "lockfile", "build-system"]
categories    = ["development-tools::build-utils", "command-line-utilities"]
```

Changes summary:
- Version: `0.1.0-dev` → `0.1.0`
- Repository: `jackmpcollins` → `rsJames-ttrpg`
- New keys: `authors`, `homepage`, `documentation`, `keywords`, `categories`
- Whitespace: aligned `=` columns for readability (the only non-content change)

- [ ] **Step 3: Verify build still succeeds**

Run: `cd /home/jackm/repos/muntjac && cargo build --locked`
Expected: PASS, no warnings about Cargo.toml.

- [ ] **Step 4: Verify all tests still pass**

Run: `cd /home/jackm/repos/muntjac && cargo test --locked`
Expected: 346+ tests pass (count from `s7b-complete`; S8b added no Rust tests).

- [ ] **Step 5: Verify `cargo publish --dry-run` succeeds**

Run: `cd /home/jackm/repos/muntjac && cargo publish --dry-run --locked 2>&1 | tee /tmp/publish-dry-run.log`
Expected: ends with `Packaged N files, X.YY MiB ...` (or similar). Most importantly: no errors about missing fields, malformed keywords, or invalid categories.

If `cargo publish` complains about the **tarball size exceeding 10 MiB**, add `exclude = ["tests/fixtures/**", "docs/**"]` to `[package]`. Re-run dry-run. If size is fine without `exclude`, do NOT add it — fixtures are useful inline documentation.

If `cargo publish` complains about a **dirty working tree**, that means uncommitted changes exist (e.g., this task's edits to Cargo.toml). The `--locked` flag with `--dry-run` should be tolerant of uncommitted changes; if it isn't, that's expected for this step before commit.

- [ ] **Step 6: Commit**

```sh
cd /home/jackm/repos/muntjac
git add Cargo.toml
git commit -m "$(cat <<'EOF'
chore(release): polish Cargo.toml metadata for v0.1.0

- Bump version 0.1.0-dev → 0.1.0
- Update repository URL: jackmpcollins → rsJames-ttrpg (matches push origin)
- Add authors, homepage, documentation, keywords, categories

keywords + categories chosen for crates.io discoverability:
- keywords: buck2, python, uv, lockfile, build-system
- categories: development-tools::build-utils, command-line-utilities

Verified `cargo publish --dry-run --locked` succeeds locally.
EOF
)"
```

---

## Task 3: CHANGELOG.md

**Files:**
- Create: `CHANGELOG.md`

- [ ] **Step 1: Verify the file does not already exist**

Run: `test ! -e /home/jackm/repos/muntjac/CHANGELOG.md && echo missing-as-expected`
Expected: `missing-as-expected`

- [ ] **Step 2: Write `CHANGELOG.md` at repo root**

Use today's ISO date (2026-05-25) in the `## [0.1.0]` heading. Exact contents:

```markdown
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
- Credible-launch surface: numpy, pandas, fastapi, requests, ruff on Linux x86_64 + macOS arm64.
- Seed community fixup registry at github.com/rsJames-ttrpg/muntjac-fixups (5 packages: pillow, cryptography, lxml, pyzmq, psycopg2-binary).

### Known limitations

- `muntjac unused`, `muntjac audit` not yet implemented (planned for v0.2+).
- Windows + Intel macOS prebuilt binaries not in this release; install via `cargo install muntjac` works on those platforms.
- See `docs/superpowers/TECH_DEBT.md` for the full ledger.

[Unreleased]: https://github.com/rsJames-ttrpg/muntjac/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/rsJames-ttrpg/muntjac/releases/tag/v0.1.0
```

- [ ] **Step 3: Sanity-test the awk extractor against the committed CHANGELOG**

Run:
```sh
cd /home/jackm/repos/muntjac
awk -v ver="0.1.0" '
  /^## \[/ { if (found) exit; if ($0 ~ "\\[" ver "\\]") { found=1; next } }
  found { print }
' CHANGELOG.md
```
Expected: prints lines from after `## [0.1.0] — 2026-05-25` up to (but not including) the next `## [` heading. Visually verify the output starts with a blank line followed by `### Added` and ends with the last "Known limitations" bullet line + trailing reference-link lines (the reference links at the bottom of the file are *not* under a `## [` heading, so they get included — acceptable for the GitHub Release body since they're useful links).

- [ ] **Step 4: Commit**

```sh
cd /home/jackm/repos/muntjac
git add CHANGELOG.md
git commit -m "$(cat <<'EOF'
docs(release): add CHANGELOG.md with [0.1.0] section

Keep-a-Changelog format. The release.yml workflow (Task 6) will
extract the `## [<version>]` block via an awk one-liner for use as
the GitHub Release body.
EOF
)"
```

---

## Task 4: `muntjac init` template URL update + test assertion

**Files:**
- Modify: `src/cli/init.rs:104-108` (the `[fixups]` block of the template)
- Modify: `tests/init.rs` (add substring assertion for the new comment)

- [ ] **Step 1: Read current init template block to confirm exact text**

Run: `sed -n '104,109p' /home/jackm/repos/muntjac/src/cli/init.rs`
Expected:
```rust
         [fixups]\n\
         registry = \"none\"\n\
         # When a community registry exists, set to \"github.com/<owner>/muntjac-fixups\"\n\
         # and run `muntjac fixups update` to pin a SHA. For air-gapped or\n\
         # pre-launch usage, use a local checkout: registry = \"file:///abs/path\".\n\
         allow_local_overrides = true\n\n\
```

- [ ] **Step 2: Replace the `[fixups]` comment block in the template**

Use the Edit tool on `src/cli/init.rs`. Replace the 5-line span (the 3-line comment between `registry = "none"` and `allow_local_overrides = true`) with the new wording.

Old block (the lines starting with `# When a community registry...`):
```rust
         # When a community registry exists, set to \"github.com/<owner>/muntjac-fixups\"\n\
         # and run `muntjac fixups update` to pin a SHA. For air-gapped or\n\
         # pre-launch usage, use a local checkout: registry = \"file:///abs/path\".\n\
```

New block:
```rust
         # Pin a community fixup registry (recommended for native deps). Example:\n\
         #   registry = \"github.com/rsJames-ttrpg/muntjac-fixups\"\n\
         # Then run `muntjac fixups update` to fetch and pin the latest SHA.\n\
         # For local checkout / offline use: registry = \"file:///abs/path\".\n\
```

The default value of `registry` stays `"none"` — do not change it. Only the comment changes.

- [ ] **Step 3: Add a substring assertion to `tests/init.rs::init_creates_starter_in_empty_dir`**

Open `tests/init.rs:9-34`. After the existing `assert!(cfg.contains("# Uncomment to include PEP 735 dependency groups"));` line (around line 24), add:

```rust
    // S8a: init template references the canonical community registry by name.
    assert!(cfg.contains("github.com/rsJames-ttrpg/muntjac-fixups"));
```

This locks the new wording so a regression that drops the registry-URL hint will fail CI.

- [ ] **Step 4: Run init tests to verify the assertion passes**

Run: `cd /home/jackm/repos/muntjac && cargo test --locked --test init`
Expected: all 4 tests pass (`init_creates_starter_in_empty_dir`, `init_detects_existing_pyproject`, `init_refuses_to_overwrite`, `init_with_force_overwrites`).

If `init_creates_starter_in_empty_dir` fails on the new assertion, the template edit in Step 2 didn't take — re-check the Edit invocation.

- [ ] **Step 5: Run inline init.rs unit tests**

Run: `cd /home/jackm/repos/muntjac && cargo test --locked --lib cli::init::tests`
Expected: 6 tests pass (the `find_pyproject` / `expand_requires_python` block, lines 253-337). These tests don't exercise the template render — they're unaffected by Step 2. Verify they still pass to confirm nothing else regressed.

- [ ] **Step 6: Commit**

```sh
cd /home/jackm/repos/muntjac
git add src/cli/init.rs tests/init.rs
git commit -m "$(cat <<'EOF'
feat(init): reference canonical community registry by name

The init template's [fixups] block now points users at the concrete
seed registry (github.com/rsJames-ttrpg/muntjac-fixups) instead of a
placeholder <owner>. Wording also clarifies the `muntjac fixups
update` workflow and keeps the file:/// escape hatch for offline use.

Default `registry = "none"` unchanged — opt-in still required.

tests/init.rs: new substring assertion locks the registry URL so a
future regression that drops the hint fails CI.
EOF
)"
```

---

## Task 5: publish-check.yml

**Files:**
- Create: `.github/workflows/publish-check.yml`

- [ ] **Step 1: Verify the file does not already exist**

Run: `test ! -e /home/jackm/repos/muntjac/.github/workflows/publish-check.yml && echo missing-as-expected`
Expected: `missing-as-expected`

- [ ] **Step 2: Write the workflow file**

Exact contents:

```yaml
name: publish-check

on:
  push:
    branches: [main]
  pull_request:

jobs:
  dry-run:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        with:
          submodules: recursive

      - uses: dtolnay/rust-toolchain@stable

      - uses: Swatinem/rust-cache@v2

      - name: cargo publish --dry-run
        run: cargo publish --dry-run --locked
```

Notes:
- Single job, single runner — publish state is platform-independent; not a coverage gap.
- `--locked` forces use of the committed `Cargo.lock` (catches lock-vs-toml drift).
- No `CARGO_REGISTRY_TOKEN` needed — `--dry-run` doesn't talk to crates.io.

- [ ] **Step 3: Validate workflow YAML parses**

Run:
```sh
python3 -c "import yaml; yaml.safe_load(open('/home/jackm/repos/muntjac/.github/workflows/publish-check.yml'))"
```
Expected: exits 0 with no output.

If `python3` lacks PyYAML, alternative:
```sh
ruby -ryaml -e "YAML.load_file('/home/jackm/repos/muntjac/.github/workflows/publish-check.yml')"
```
Either form catches syntax errors.

- [ ] **Step 4: Commit**

```sh
cd /home/jackm/repos/muntjac
git add .github/workflows/publish-check.yml
git commit -m "$(cat <<'EOF'
ci: add publish-check workflow (cargo publish --dry-run gate)

Single-purpose workflow runs on every PR + push to main. Catches
missing LICENSE, malformed keywords, invalid categories, oversized
tarball, missing required Cargo.toml fields — anything that would
make `cargo publish` fail at tag-cut time.

Ubuntu-only because publish state is platform-independent (no
coverage gap). See feedback_ci_no_arch_polymorphism in memory for the
principle.
EOF
)"
```

---

## Task 6: release.yml

**Files:**
- Create: `.github/workflows/release.yml`

- [ ] **Step 1: Verify the file does not already exist**

Run: `test ! -e /home/jackm/repos/muntjac/.github/workflows/release.yml && echo missing-as-expected`
Expected: `missing-as-expected`

- [ ] **Step 2: Write the workflow file**

Exact contents:

```yaml
name: release

on:
  push:
    tags: ['v*']

jobs:
  publish-crate:
    name: publish to crates.io
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        with:
          submodules: recursive

      - uses: dtolnay/rust-toolchain@stable

      - uses: Swatinem/rust-cache@v2

      - name: cargo publish --dry-run
        run: cargo publish --dry-run --locked

      - name: cargo publish
        run: cargo publish --locked
        env:
          CARGO_REGISTRY_TOKEN: ${{ secrets.CARGO_REGISTRY_TOKEN }}

  build-binaries:
    name: build ${{ matrix.target }}
    strategy:
      fail-fast: false
      matrix:
        include:
          - runner: ubuntu-latest
            target: x86_64-unknown-linux-gnu
            asset: muntjac-x86_64-unknown-linux-gnu.tar.gz
          - runner: ubuntu-24.04-arm
            target: aarch64-unknown-linux-gnu
            asset: muntjac-aarch64-unknown-linux-gnu.tar.gz
          - runner: macos-latest
            target: aarch64-apple-darwin
            asset: muntjac-aarch64-apple-darwin.tar.gz
    runs-on: ${{ matrix.runner }}
    steps:
      - uses: actions/checkout@v4
        with:
          submodules: recursive

      - uses: dtolnay/rust-toolchain@stable
        with:
          targets: ${{ matrix.target }}

      - uses: Swatinem/rust-cache@v2

      - name: cargo build --release
        run: cargo build --release --locked --target ${{ matrix.target }}

      - name: tar the binary
        run: |
          set -euo pipefail
          mkdir -p dist
          tar -czf "dist/${{ matrix.asset }}" -C "target/${{ matrix.target }}/release" muntjac

      - uses: actions/upload-artifact@v4
        with:
          name: ${{ matrix.asset }}
          path: dist/${{ matrix.asset }}

  github-release:
    name: create GitHub Release
    needs: [publish-crate, build-binaries]
    runs-on: ubuntu-latest
    permissions:
      contents: write
    steps:
      - uses: actions/checkout@v4

      - uses: actions/download-artifact@v4
        with:
          path: dist
          merge-multiple: true

      - name: extract CHANGELOG section for this tag
        run: |
          set -euo pipefail
          tag="${GITHUB_REF_NAME#v}"
          awk -v ver="$tag" '
            /^## \[/ { if (found) exit; if ($0 ~ "\\[" ver "\\]") { found=1; next } }
            found { print }
          ' CHANGELOG.md > release-notes.md
          echo "--- extracted release notes ---"
          cat release-notes.md

      - name: gh release create
        env:
          GH_TOKEN: ${{ secrets.GITHUB_TOKEN }}
        run: gh release create "$GITHUB_REF_NAME" dist/* --title "$GITHUB_REF_NAME" --notes-file release-notes.md
```

Notes:
- Three jobs: `publish-crate`, `build-binaries` (matrix), `github-release` (depends on both).
- If `publish-crate` fails (e.g., transient crates.io error), `github-release` is skipped — avoids the half-state.
- `permissions: contents: write` on `github-release` is required for `gh release create`.
- `CARGO_REGISTRY_TOKEN` is a manual repo-secret prereq; the workflow assumes it exists. If missing at tag-cut time, the `cargo publish` step fails fast and the maintainer retags as v0.1.1 (cargo versions are immutable — see spec §5.5 step 6).

- [ ] **Step 3: Validate workflow YAML parses**

Run:
```sh
python3 -c "import yaml; yaml.safe_load(open('/home/jackm/repos/muntjac/.github/workflows/release.yml'))"
```
Expected: exits 0 with no output.

- [ ] **Step 4: Smoke-test the awk extractor inline (since release.yml can't run until a tag is pushed)**

Run:
```sh
cd /home/jackm/repos/muntjac
GITHUB_REF_NAME=v0.1.0
tag="${GITHUB_REF_NAME#v}"
awk -v ver="$tag" '
  /^## \[/ { if (found) exit; if ($0 ~ "\\[" ver "\\]") { found=1; next } }
  found { print }
' CHANGELOG.md
```
Expected: prints the `## [0.1.0] — 2026-05-25` block contents (everything from after that heading to just before the next `## [` heading — `## [Unreleased]` is *above*, so the awk run on `## [0.1.0]` walks down to EOF and emits the body of that section plus the trailing reference-link lines). Visually verify the output is sane release-notes text.

- [ ] **Step 5: Commit**

```sh
cd /home/jackm/repos/muntjac
git add .github/workflows/release.yml
git commit -m "$(cat <<'EOF'
ci: add release workflow (cargo publish + cross-compiled GH binaries)

Triggers on v* tag push. Three jobs in dependency order:

1. publish-crate — cargo publish --dry-run then cargo publish.
2. build-binaries — cross-compile to linux-x86_64, linux-aarch64,
   macos-aarch64. All three are native builds on their runners.
3. github-release — extracts the matching CHANGELOG section via awk
   and creates the GH Release with binaries attached.

The Release page is only created if both publish-crate and all three
binary builds succeed — avoids the "on crates.io but not GH Release"
or vice-versa half-state.

CARGO_REGISTRY_TOKEN is a manual repo-secret prereq, configured by
the maintainer before tag-push (spec §5.5 step 3).
EOF
)"
```

---

## Task 7: demo.yml

**Files:**
- Create: `.github/workflows/demo.yml`

- [ ] **Step 1: Verify the file does not already exist**

Run: `test ! -e /home/jackm/repos/muntjac/.github/workflows/demo.yml && echo missing-as-expected`
Expected: `missing-as-expected`

- [ ] **Step 2: Write the workflow file**

This is the largest single file in S8a. It reuses two snippets verbatim from `ci.yml`:
- `install cp312 python` step (ci.yml lines 39-54)
- `install buck2` step (ci.yml lines 56-80)

Exact contents:

```yaml
name: demo

on:
  push:
    branches: [main]
  pull_request:

jobs:
  demo:
    name: ${{ matrix.runner }}
    strategy:
      fail-fast: false
      matrix:
        runner: [ubuntu-latest, ubuntu-24.04-arm, macos-latest]
    runs-on: ${{ matrix.runner }}
    steps:
      - uses: actions/checkout@v4
        with:
          submodules: recursive

      - uses: dtolnay/rust-toolchain@stable

      - uses: Swatinem/rust-cache@v2

      - name: install cp312 python
        run: |
          set -euo pipefail
          # uv ships a known-good cp312 across runners.
          if ! command -v uv >/dev/null 2>&1; then
            python3 -m pip install --user uv || pipx install uv
          fi
          uv python install 3.12
          UV_PY_BIN="$(uv python find 3.12)"
          # Symlink so the prelude's system_python_toolchain finds `python3.12`.
          if [ "${{ runner.os }}" = "macOS" ]; then
            ln -sf "$UV_PY_BIN" /usr/local/bin/python3.12
          else
            sudo ln -sf "$UV_PY_BIN" /usr/local/bin/python3.12
          fi
          python3.12 --version

      - name: install buck2
        run: |
          set -euo pipefail
          BUCK2_RELEASE="2026-05-18"
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
            if [ "${{ runner.os }}" = "macOS" ]; then
              brew install zstd
            else
              sudo apt-get update && sudo apt-get install -y zstd
            fi
          fi
          zstd -d /tmp/buck2.zst -o "$HOME/.local/bin/buck2"
          chmod +x "$HOME/.local/bin/buck2"
          echo "$HOME/.local/bin" >> "$GITHUB_PATH"
          "$HOME/.local/bin/buck2" --version

      - name: install muntjac under test
        run: cargo install --path . --locked

      - name: run the README demo end-to-end
        run: |
          set -euo pipefail

          # Fixture 02 provides battle-tested Buck2 scaffolding (prelude submodule,
          # toolchains/, .buckconfig, PACKAGE). We copy those in to represent
          # "the user's existing Buck2 project setup" — those files are NOT
          # muntjac's responsibility to scaffold (see spec §5.2 prelude note,
          # and TD item for `muntjac init` scaffolding `.buckconfig`).
          FIXTURE="$(pwd)/tests/fixtures/buck/02-numpy-pandas"
          DEMO=$(mktemp -d)
          cd "$DEMO"

          # Copy Buck2 scaffolding (NOT the muntjac state — we're testing init).
          cp -r "$FIXTURE/prelude"     ./prelude
          cp -r "$FIXTURE/toolchains"  ./toolchains
          cp    "$FIXTURE/.buckconfig" ./.buckconfig
          cp    "$FIXTURE/PACKAGE"     ./PACKAGE

          # Fresh uv project (the README's starting point).
          uv init --bare --name muntjac-demo
          uv add 'numpy>=2.1,<2.3'

          # muntjac flow: init → vendor → buckify.
          muntjac init
          muntjac vendor
          muntjac buckify

          # Write a smoke target. python_binary depending on numpy; modifiers
          # point at the config target muntjac emits for cp312.
          mkdir -p app
          cat > app/main.py <<'PY'
          import numpy as np
          arr = np.zeros(3)
          print("DEMO_OK shape={} dtype={}".format(arr.shape, arr.dtype))
          PY
          cat > app/BUCK <<'BUCK'
          python_binary(
              name = "main",
              main = "main.py",
              modifiers = ["//third-party/python/config:py312"],
              deps = ["//third-party/python:numpy"],
          )
          BUCK

          buck2 run //app:main 2>&1 | tee /tmp/demo-out.txt
          grep -F "DEMO_OK shape=(3,)" /tmp/demo-out.txt
```

Notes:
- `uv init --bare --name muntjac-demo` produces a minimal `pyproject.toml` without an example src tree. The `--name` flag avoids the auto-derived-from-cwd name (which is a tmpdir suffix and not portable).
- The smoke target uses *only numpy* (not pandas/requests) to keep the buck2 wheel-install surface small. The point of the demo is to prove the muntjac flow works end-to-end, not to import every package added by `uv add`. Adding more imports later if a regression slips past numpy-only is trivial.
- The `cp -r prelude` step copies the prelude submodule (which the outer `actions/checkout@v4` with `submodules: recursive` already initialized).
- Pinning `numpy>=2.1,<2.3` matches fixture 02's pyproject so the wheel set is known-stable. Without pinning, `uv add numpy` would resolve to whatever's latest on PyPI, risking flakiness.

- [ ] **Step 3: Validate workflow YAML parses**

Run:
```sh
python3 -c "import yaml; yaml.safe_load(open('/home/jackm/repos/muntjac/.github/workflows/demo.yml'))"
```
Expected: exits 0 with no output.

- [ ] **Step 4: Verify the inline BUCK + main.py expansion doesn't get HEREDOC-escaped weirdly**

Run:
```sh
awk '/cat > app\/BUCK/,/^          BUCK$/' /home/jackm/repos/muntjac/.github/workflows/demo.yml
```
Expected: prints exactly the `cat > app/BUCK <<'BUCK' ... BUCK` block. The `'BUCK'` quoting on the heredoc delimiter prevents shell expansion inside the body (we want `//third-party/python:numpy` to land literally, not be interpreted).

- [ ] **Step 5: Commit**

```sh
cd /home/jackm/repos/muntjac
git add .github/workflows/demo.yml
git commit -m "$(cat <<'EOF'
ci: add demo workflow (60-second README flow end-to-end)

Runs the README's quickstart commands in a tmpdir on every matrix
runner (ubuntu-latest, ubuntu-24.04-arm, macos-latest):

1. Copy fixture 02's prelude/toolchains/.buckconfig/PACKAGE as
   "the user's existing Buck2 project scaffolding."
2. uv init --bare && uv add numpy.
3. muntjac install (via cargo install --path .).
4. muntjac init → vendor → buckify.
5. Write app/main.py and app/BUCK.
6. buck2 run //app:main, grep for DEMO_OK substring.

Runs on all three runners (no arch-conditional skips) per
feedback_ci_no_arch_polymorphism — establishes the right pattern
going forward.

Bootstrap snippets (install cp312 python, install buck2) are
duplicated from ci.yml for now. DRY refactor can come post-launch
once the surface stabilises.
EOF
)"
```

---

## Task 8: README.md rewrite

**Files:**
- Modify: `README.md` (full rewrite — current 12-line stub becomes ~180-line entry point)

- [ ] **Step 1: Verify current state**

Run: `wc -l /home/jackm/repos/muntjac/README.md`
Expected: 13 (the stub).

- [ ] **Step 2: Write the new README**

Replace the file's contents entirely with this:

````markdown
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

v0.1.0. Linux x86_64 + macOS arm64 are the credible-launch platforms — numpy, pandas, fastapi, requests, and ruff are confirmed working end-to-end. Linux arm64 is supported and tested in CI. Windows + Intel macOS work via `cargo install muntjac` (compiles from source); prebuilt binaries are planned post-v0.1.0.

See [roadmap](docs/superpowers/specs/2026-05-20-muntjac-roadmap.md) for v0.2+ plans (vendor mode, audit, multi-tree).

## Contributing

Issues + PRs welcome. See [CONTRIBUTING.md](CONTRIBUTING.md) once it lands; in the meantime, see [`docs/superpowers/specs/`](docs/superpowers/specs/) for the design specs and [`docs/superpowers/TECH_DEBT.md`](docs/superpowers/TECH_DEBT.md) for the open ledger.

## License

MIT — see [LICENSE](LICENSE).
````

- [ ] **Step 3: Verify rendering**

Run: `wc -l /home/jackm/repos/muntjac/README.md`
Expected: roughly 100-130 lines. Anything outside 80-200 lines is a red flag (too thin or too verbose).

Spot-check structure:
```sh
grep -E '^#+' /home/jackm/repos/muntjac/README.md
```
Expected headings in order:
- `# muntjac`
- `## What is muntjac?`
- `## Quickstart`
- `## Setting up Buck2`
- `## Configuration`
- `## Community fixups`
- `## Status`
- `## Contributing`
- `## License`

- [ ] **Step 4: Verify all internal markdown links resolve to real files**

Run:
```sh
cd /home/jackm/repos/muntjac
grep -oE '\(docs/[^)]+\)' README.md | tr -d '()'
```
Expected: each printed path corresponds to a real file. Verify:
- `docs/superpowers/specs/2026-05-20-muntjac-design.md` — exists
- `docs/superpowers/specs/2026-05-20-muntjac-roadmap.md` — exists
- `docs/superpowers/specs/` — exists (directory)
- `docs/superpowers/TECH_DEBT.md` — exists

The `CONTRIBUTING.md` link is intentionally to a non-existent file — it's a forward-compatible placeholder, called out in spec §2.3.

- [ ] **Step 5: Commit**

```sh
cd /home/jackm/repos/muntjac
git add README.md
git commit -m "$(cat <<'EOF'
docs(readme): rewrite as canonical project entry point

Replaces the 12-line stub with a quickstart + pointers layout:

- 4-paragraph "What is muntjac?" (problem / solution / moat / DX).
- Quickstart block matching the CI demo flow exactly.
- "Setting up Buck2" section pointing new users at fixture 02 as a
  starter template.
- Configuration overview (one screen of muntjac.toml highlights).
- Community fixups section linking to muntjac-fixups.
- Status (credible-launch platforms + Windows/Intel-macOS note).
- Forward-compat CONTRIBUTING.md link (file is a follow-up, see
  spec §2.3).

Badges: CI, crates.io version, license.
EOF
)"
```

---

## Task 9: docs/launch-post.md

**Files:**
- Create: `docs/launch-post.md`

- [ ] **Step 1: Verify the file does not already exist**

Run: `test ! -e /home/jackm/repos/muntjac/docs/launch-post.md && echo missing-as-expected`
Expected: `missing-as-expected`

- [ ] **Step 2: Write the file**

Exact contents:

````markdown
# muntjac v0.1.0 — launch post (draft)

Three audience-tuned drafts. Maintainer picks one (or more) and posts manually after tag-cut. **Do not auto-post via CI** — the post itself is reversible only with edits, and timing should align with the maintainer's bandwidth for responses.

---

## Short form (Buck2 Discord, Twitter/X)

> muntjac v0.1.0: translate `uv.lock` into Buck2 build rules. numpy/pandas/fastapi/requests/ruff working end-to-end on Linux x86_64 + macOS arm64. Community fixup registry for native deps at github.com/rsJames-ttrpg/muntjac-fixups.
>
> `cargo install muntjac` — README has the 60-second demo. Feedback welcome.

---

## Medium form (Reddit r/Python, lobste.rs)

> **muntjac** is a Rust CLI that translates `uv.lock` (Python's modern lockfile) into Buck2 build rules. v0.1.0 ships today.
>
> **Why:** Buck2 users with Python in their monorepo have had to either hand-write `pypi_package` rules or use Reindeer (which is Cargo-specific). muntjac fills the uv-to-Buck gap, with first-class support for the parts that hurt: PEP 503 normalization, marker evaluation per platform, PEP 517 sdist prebake, native-extension fixups.
>
> **DX:** Keep `uv add` / `uv lock` / `uv sync` for local dev — muntjac re-derives Buck rules from `uv.lock` on demand. No hand-written `pypi_package` rules, no wheel-filename debugging, no marker-eval-by-hand. Flattens Buck2's learning curve for Python teams: uv-native ergonomics in, Buck-native targets out.
>
> **Demo:**
> ```
> cargo install muntjac
> muntjac init && uv add numpy pandas requests
> muntjac vendor && muntjac buckify
> buck2 run //my:target
> ```
>
> **Moat:** Community fixup registry at github.com/rsJames-ttrpg/muntjac-fixups — seed includes pillow, cryptography, lxml, pyzmq, psycopg2-binary. PRs welcome.
>
> **Known limits:** Linux x86_64 + macOS arm64 are the credible-launch platforms. Windows + Intel macOS work via `cargo install` but no prebuilt binaries yet. See ROADMAP for v0.2+.

---

## Long form (HN Show HN)

Hand-write per the moment. Use the medium-form bullets as outline. HN audience cares about engineering choices: why Rust, why a separate fixup repo, what was hard.

**Do not post until after at least one external user has confirmed the demo works on a clean machine.** HN front-page traffic on a broken README is brutal.

Discussion hooks the maintainer should be ready for:
- "Why not extend Reindeer?" — Reindeer is Cargo-shaped; uv.lock has different invariants (per-platform wheel selection, PEP 503 normalization, dependency groups). Forking would have been a rewrite.
- "Why Buck2 over Bazel?" — Bazel has rules_python; Buck2 doesn't have a comparable Python story. muntjac fills that gap.
- "How does this compare to `pip-tools` / `poetry-export` + custom rules?" — Those flow through requirements.txt, losing the resolver's per-platform marker eval. uv.lock is the strict superset; muntjac preserves it.
- "What's the fixup registry actually doing?" — It's a layered patch system on top of resolver output. Community fixups handle the libjpeg/openssl/libzmq incantations once, locally; downstream users just turn on the registry pointer.

Pin a comment at the top of the HN post with a link to the GitHub Releases page for direct binary downloads (otherwise readers will keep asking "do I need to compile this?").
````

- [ ] **Step 3: Verify file contents are sane**

Run: `grep -E '^##' /home/jackm/repos/muntjac/docs/launch-post.md`
Expected three section headings:
```
## Short form (Buck2 Discord, Twitter/X)
## Medium form (Reddit r/Python, lobste.rs)
## Long form (HN Show HN)
```

- [ ] **Step 4: Commit**

```sh
cd /home/jackm/repos/muntjac
git add docs/launch-post.md
git commit -m "$(cat <<'EOF'
docs: add launch-post stub for v0.1.0

Three audience-tuned drafts: short (Discord / Twitter), medium
(Reddit / lobste.rs), long (HN Show HN). The HN draft is a writing
prompt + discussion-hook list rather than a finished post — HN
demands hand-tuning per the moment.

Maintainer posts manually after tag-cut. CI does not auto-post.
EOF
)"
```

---

## Task 10: Stage-close — mark S8a shipped in roadmap

**Files:**
- Modify: `docs/superpowers/specs/2026-05-20-muntjac-roadmap.md` (the S8a row at line ~191)
- Modify: `docs/superpowers/specs/2026-05-20-muntjac-roadmap.md` (the spec/plan/status table at line ~290)

**Run this task AFTER tasks 1-9 have all been merged.** Marks the stage shipped with the commit count.

- [ ] **Step 1: Count S8a-attributable commits since the previous tag**

Run:
```sh
cd /home/jackm/repos/muntjac
git log --oneline $(git describe --tags --abbrev=0)..HEAD | wc -l
```
Record the count (N). Expected ~11: 2 brainstorm-phase commits (S8a spec + DX-addition follow-up, both landed before Task 1) plus 9 implementation commits (Tasks 1-9). Final count in the "Shipped" line will be N+1 to include this Task 10 commit itself.

- [ ] **Step 2: Read current roadmap state for the S8a section**

Run: `sed -n '191,205p' /home/jackm/repos/muntjac/docs/superpowers/specs/2026-05-20-muntjac-roadmap.md`
Expected: the existing `#### S8a — muntjac repo polish & v0.1.0` block (status ⬜ blocked on S8b). Confirm the heading line text exactly.

- [ ] **Step 3: Update the S8a section to mark shipped**

Use the Edit tool. Replace the heading line:

```
#### S8a — muntjac repo polish & v0.1.0
```

with:

```
#### S8a — muntjac repo polish & v0.1.0 ✅ shipped
```

Then immediately below the "**Touches:**" line of the S8a section, add:

```

**Shipped:** (N+1) commits, tag `s8a-complete`. README rewrite (canonical entry point, badges, 60-second quickstart), 60-second demo CI workflow (all 3 runners), Cargo.toml polish (v0.1.0, authors, keywords, categories, repository → rsJames-ttrpg), LICENSE + CHANGELOG, publish-check.yml + release.yml workflows (release.yml lints standalone — runs at tag-cut), `muntjac init` template pointing at canonical seed registry, launch-post stub with three audience drafts. crates.io publish + GitHub Release creation gated on the maintainer cutting `v0.1.0` tag with `CARGO_REGISTRY_TOKEN` configured.
```

(Where `N` is the count from Step 1.)

- [ ] **Step 4: Update the spec/plan/status table at the bottom of the roadmap**

Find the S8a row (around line 290):

```
| S8a | (not yet written) | (not yet written) | ⬜ blocked on S8b |
```

Replace with:

```
| S8a | [2026-05-25-muntjac-s8a-launch-polish-design.md](./2026-05-25-muntjac-s8a-launch-polish-design.md) | [2026-05-25-muntjac-s8a-launch-polish.md](../plans/2026-05-25-muntjac-s8a-launch-polish.md) | ✅ shipped (tag `s8a-complete`, (N+1) commits) |
```

(Where `N` is the count from Step 1.)

- [ ] **Step 5: Verify roadmap renders**

Run: `grep -A2 '^#### S8a' /home/jackm/repos/muntjac/docs/superpowers/specs/2026-05-20-muntjac-roadmap.md`
Expected: heading is now `#### S8a — muntjac repo polish & v0.1.0 ✅ shipped`.

Run: `grep '| S8a |' /home/jackm/repos/muntjac/docs/superpowers/specs/2026-05-20-muntjac-roadmap.md`
Expected: the row links to the design + plan and shows `✅ shipped`.

- [ ] **Step 6: Commit + tag**

```sh
cd /home/jackm/repos/muntjac
git add docs/superpowers/specs/2026-05-20-muntjac-roadmap.md
git commit -m "$(cat <<'EOF'
docs(s8a): mark ✅ shipped in roadmap

S8a covered: README rewrite, LICENSE, CHANGELOG, Cargo.toml polish,
demo.yml + publish-check.yml + release.yml workflows, muntjac init
template URL update, launch-post stub. CI green; tag-cut + cargo
publish + gh release create are manual maintainer actions, executed
when the maintainer is ready.
EOF
)"
git tag -a s8a-complete -m "S8a — launch polish complete (pre-v0.1.0-tag)"
git push origin main
git push origin s8a-complete
```

**Do NOT push the `v0.1.0` tag here.** That's a separate maintainer-driven action — see spec §5.5 (the tag-cut flow). `s8a-complete` is the stage marker; `v0.1.0` is the release marker that triggers `release.yml`.

---

## Self-review checklist

After all tasks complete, the maintainer (separately from this plan's CI gates) walks through the spec §5.5 tag-cut flow to actually publish.

**Spec coverage check** — every spec section maps to at least one task:
- §1 (goal + in/out scope + exit criteria) — covered by all tasks collectively
- §2 (README) — Task 8
- §3 (Cargo.toml) — Task 2
- §4.1 (LICENSE) — Task 1
- §4.2 (CHANGELOG) — Task 3
- §4.3 (CHANGELOG extractor) — Task 6 step 4 (smoke-test inline)
- §5.1 (publish-check.yml) — Task 5
- §5.2 (demo.yml) — Task 7
- §5.3 (release.yml) — Task 6
- §5.4 (muntjac init template) — Task 4
- §5.5 (tag-cut flow) — out of plan scope; lives in spec as maintainer checklist
- §6 (launch post) — Task 9
- §7 (risks) — addressed implicitly by Tasks 2-7 mitigations
- §8 (plan touch list) — this whole plan

**No spec requirement is missing a task.**

**Type / identifier consistency:** No new Rust types or method signatures introduced. The init template's `[fixups]` block keeps `registry = "none"` default and `allow_local_overrides = true` default — unchanged from current state. The CHANGELOG extractor's `awk` invocation is identical across Task 3 step 3 (sanity test) and Task 6 step 2 (release.yml embed) and Task 6 step 4 (smoke-test).

**Test re-baselines:**
- Task 4 adds one new substring assertion to `tests/init.rs`. No insta snapshots involved.
- All other tasks are declarative artifacts whose verification is "file exists" + "YAML parses" + (for Task 2) "cargo publish --dry-run succeeds".
