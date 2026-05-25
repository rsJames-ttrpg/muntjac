# S8a — muntjac repo polish & v0.1.0 launch

**Status:** Designed 2026-05-25. Pending implementation plan.

**Predecessor:** S8b (`muntjac-fixups` seed repo, shipped 2026-05-24, tag `s8b-complete` / `seed-v0.1.0`).

**Successor:** Phase 2 (S9 vendor mode, S10 audit/unused, S11 multi-tree) — none v0.1.0-blocking.

---

## 1. Goal

Polish the muntjac repo for public consumption and ship v0.1.0 to crates.io + GitHub Releases. After this stage, a user who has never heard of muntjac can read the README, follow the demo, and have a working Buck2 build of a Python program in five minutes.

### 1.1 In scope

| Deliverable | Where it lands |
|-------------|----------------|
| README rewrite (quickstart + pointers, ~150–250 lines) | `README.md` |
| `LICENSE` file (MIT text, copyright Jack Mayo) | `LICENSE` |
| `CHANGELOG.md` with `## [0.1.0]` block (Keep-a-Changelog format) | `CHANGELOG.md` |
| `Cargo.toml` metadata polish (version, repository, authors, keywords, categories, homepage, documentation) | `Cargo.toml` |
| `muntjac init` template: concrete registry URL + clearer comment | `src/cli/init.rs` |
| 60-second demo as CI workflow on all three matrix runners | `.github/workflows/demo.yml` |
| Dedicated `cargo publish --dry-run` gate | `.github/workflows/publish-check.yml` |
| Tag-triggered release workflow: dry-run → publish → cross-compile → GitHub Release | `.github/workflows/release.yml` |
| Launch post stub (short / medium / long form drafts) | `docs/launch-post.md` |
| Roadmap mark-shipped | `docs/superpowers/specs/2026-05-20-muntjac-roadmap.md` |

### 1.2 Out of scope (deferred, logged in TECH_DEBT)

| Item | Reason | Tech debt entry |
|------|--------|-----------------|
| Windows + macos-x86_64 prebuilt binaries | Credible-launch surface is Linux x86_64 + macOS arm64; Windows is untested | TD-S8a-02 |
| `cargo-binstall` metadata block | Auto-detection works; explicit config is polish | TD-S8a-03 |
| Fix existing fixtures 04/05 ubuntu-only CI gating | Pre-existing gap; S8a establishes the *new* pattern (demo.yml runs on all runners) without rewriting old fixtures | TD-S8a-01 |
| Pressing `cargo publish` / posting the launch post | Deliberately manual — human-in-loop on irreversible side-effects | — |
| Re-baselining old fixture snapshots, retroactive workflow cleanups | Not in scope; S8a adds, doesn't refactor | — |

### 1.3 Exit criteria

1. README demo passes on all three CI matrix runners (`ubuntu-latest`, `ubuntu-24.04-arm`, `macos-latest`) via `demo.yml`.
2. `cargo publish --dry-run --locked` passes on every PR/push via `publish-check.yml`.
3. `release.yml` exists, configured to trigger on `v*` tags, and has been smoke-validated by a maintainer (read-through + lint, since actual execution requires tag-cut).
4. `CARGO_REGISTRY_TOKEN` is configured in repo secrets (manual maintainer prereq; checked at tag-cut time, not by CI).
5. `Cargo.toml.version == "0.1.0"`, `CHANGELOG.md` has a `## [0.1.0]` block dated 2026-05-25 (or close), repository URL is `rsJames-ttrpg`.
6. `docs/launch-post.md` exists with three audience-tuned drafts.
7. `LICENSE` file exists at repo root.
8. Roadmap S8a row marked ✅ shipped with commit count.

The actual tag-cut, `cargo publish`, `gh release create`, and launch post — manual maintainer actions, executed once exit criteria above are met. Not gated by CI; their success is verified by the maintainer via the tag-cut checklist (§5.2).

---

## 2. README rewrite

### 2.1 Structure

Target ~150–250 lines, in this order:

1. **Title + tagline + badges** — 1 line each:
   - Title: `# muntjac`
   - Tagline: `Translate uv.lock into Buck2 build rules.`
   - Badges: CI status (existing ci.yml), crates.io version, license
2. **What is muntjac?** — 4 paragraphs:
   - (a) Problem: `uv` doesn't speak Buck. Hand-writing `pypi_package` rules per dep is brittle.
   - (b) Solution: muntjac reads `uv.lock` + a `muntjac.toml` config, emits `BUCK` + `muntjac.bzl` + `wiring.bzl` + `config/BUCK`. PEP 503 normalization, marker eval per platform, PEP 517 sdist prebake, native-extension fixups all handled.
   - (c) Moat: community fixup registry at `github.com/rsJames-ttrpg/muntjac-fixups` so users don't write the libjpeg / openssl / libzmq incantations every time.
   - (d) Developer experience: muntjac is designed to flatten Buck2's learning curve for Python teams. Keep using `uv add` / `uv lock` / `uv sync` for day-to-day dependency work — muntjac re-derives Buck rules from `uv.lock` on demand. The mental model stays "edit pyproject.toml → re-buckify"; you never hand-edit `pypi_package` rules, never look up wheel filenames, never debug marker eval by hand. uv-native ergonomics in, Buck-native targets out.
3. **Quickstart** — verbatim demo block (5 lines, copy-pasteable):
   ```sh
   cargo install muntjac
   cd my-py-project           # has a pyproject.toml
   muntjac init               # writes muntjac.toml + third-party/python/ skeleton
   uv add numpy pandas requests
   muntjac vendor             # prebake pure-python sdists
   muntjac buckify            # emit Buck rules
   buck2 run //my:target
   ```
4. **Configuration** — one screen of `muntjac.toml` highlights with inline `#` comments. Don't repeat the full schema; link to the design spec for the rest.
5. **Community fixups** — explain layering (community + local + escape hatches), how to opt in (`registry = "github.com/rsJames-ttrpg/muntjac-fixups"`), link to the seed repo's README + CONTRIBUTING.
6. **Status** — One paragraph: "v0.1.0; Linux x86_64 + macOS arm64 are the credible-launch platforms. Windows + Intel macOS work via `cargo install` (compiles from source); prebuilt binaries planned post-v0.1.0." Link to roadmap.
7. **Contributing + License** — `CONTRIBUTING.md` placeholder (file does not yet exist; **§2.3 marks this as a follow-up**) + LICENSE link.

### 2.2 What stays out

- Full `muntjac.toml` schema reference (link to design spec).
- Full fixup-authoring tutorial (link to muntjac-fixups CONTRIBUTING.md).
- Troubleshooting / FAQ (no signal yet; add post-launch).
- Architecture diagrams (verbal description is enough at v0.1.0).

### 2.3 CONTRIBUTING.md

Not created in S8a. README links to a `CONTRIBUTING.md` placeholder; the file itself can be added as a separate small-PR follow-up (logged as a TODO in the plan, not a TECH_DEBT entry — a missing CONTRIBUTING is an obvious gap that doesn't need a ledger). The link is included so the README is forward-compatible.

---

## 3. Cargo.toml metadata

### 3.1 Final state of `[package]` table

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

### 3.2 Changes from current state

- **Bump:** `version = "0.1.0-dev"` → `"0.1.0"`.
- **Re-target:** `repository = "https://github.com/jackmpcollins/muntjac"` → `"https://github.com/rsJames-ttrpg/muntjac"`.
- **New:** `authors`, `homepage`, `documentation`, `keywords`, `categories`.

### 3.3 Constraints

- `keywords`: max 5 entries, max 20 chars each, alphanumeric + `-` / `_`. Current picks fit.
- `categories`: must match crates.io's canonical slug list. `development-tools::build-utils` and `command-line-utilities` are valid; verified at <https://crates.io/category_slugs>.
- `documentation = docs.rs/muntjac`: docs.rs auto-builds `cargo doc` for the lib target. The CLI binary won't have great docs.rs coverage but the lib (used by integration tests) will. Setting the field is conventional even when the binary is the primary surface.

### 3.4 Tarball size

`cargo publish --dry-run` reports the packaged tarball size. If `tests/fixtures/**` or `docs/**` push past crates.io's 10 MiB soft limit, add:

```toml
exclude = ["tests/fixtures/**", "docs/**"]
```

Verify during publish-check.yml's first run. If size is fine without `exclude`, don't add it — the fixtures double as documentation for downstream maintainers reading the source crate.

---

## 4. LICENSE + CHANGELOG

### 4.1 LICENSE file

New file at repo root, standard MIT text:

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

Required for: GitHub's License-detection sidebar, downstream tools (cargo-deny, FOSSA) that prefer a real file over a Cargo.toml string, and `cargo publish`'s preference for a `LICENSE` file in the packaged tarball.

### 4.2 CHANGELOG.md (Keep-a-Changelog)

New file at repo root:

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

### 4.3 CHANGELOG extraction

The release workflow extracts the `## [<version>]` block as the GitHub Release body via an `awk` one-liner — no new tooling needed:

```sh
tag="${GITHUB_REF_NAME#v}"
awk -v ver="$tag" '
  /^## \[/ { if (found) exit; if ($0 ~ "\\[" ver "\\]") { found=1; next } }
  found { print }
' CHANGELOG.md > release-notes.md
```

Sanity-tested during S8a implementation by running the snippet locally against the committed CHANGELOG. Not a unit-tested helper — a one-time stdout check is sufficient.

---

## 5. CI workflows + release flow

### 5.1 `.github/workflows/publish-check.yml` (dedicated dry-run gate)

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
        with: { submodules: recursive }
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - run: cargo publish --dry-run --locked
```

Single-purpose, no arch matrix (publish state is platform-independent so this isn't a coverage gap — see [[feedback_ci_no_arch_polymorphism]] in memory). Catches missing `LICENSE`, malformed keywords, oversized tarball, missing required fields *before* tag-cut.

### 5.2 `.github/workflows/demo.yml` (60-second README demo)

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
        with: { submodules: recursive }
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2

      - name: install cp312 python
        # Verbatim copy of the snippet in .github/workflows/ci.yml lines 39-54
        # (the `install cp312 python` step). Plan task duplicates the exact YAML.
        run: |
          # ... see ci.yml lines 39-54

      - name: install buck2
        # Verbatim copy of the snippet in .github/workflows/ci.yml lines 56-80
        # (the `install buck2` step). Plan task duplicates the exact YAML.
        run: |
          # ... see ci.yml lines 56-80

      - name: install muntjac under test
        run: cargo install --path . --locked

      - name: run the README demo
        run: |
          set -euo pipefail
          DEMO=$(mktemp -d)
          cd "$DEMO"
          uv init demo --bare
          cd demo
          uv add numpy pandas requests
          muntjac init
          muntjac vendor
          muntjac buckify
          # The README demo continues with `buck2 run`; demo.yml mirrors that.
          # A trivial BUCK file + smoke target is written here, then:
          # buck2 run //tests/smoke:demo
          # Asserts the run completes; output check is "ok if no crash."
```

Notes:
- Runs on **all three matrix runners** — establishes the right pattern from day one.
- `cargo install --path . --locked` installs the muntjac under test (the one in this checkout), not a published version. Fresh-machine `cargo install muntjac` path is implicitly validated post-tag by the maintainer during the smoke-test step of §5.5.
- Prelude handling: the README documents that users need to clone facebook/buck2-prelude into their project and set `[repositories] prelude = ...` in `.buckconfig`. demo.yml does the same. This is **path A** of the prelude question raised in brainstorming — extending `muntjac init` to scaffold `.buckconfig` is out of scope for S8a, captured as a TODO in the plan but not blocking launch.
- The `buck2 run` step writes a minimal `BUCK` file inline (within the workflow YAML) — a `python_binary` that imports `numpy` and prints its version. Smoke target lives in the tmpdir, not committed.

### 5.3 `.github/workflows/release.yml` (tag-triggered, three jobs)

```yaml
name: release
on:
  push:
    tags: ['v*']

jobs:
  publish-crate:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        with: { submodules: recursive }
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - run: cargo publish --dry-run --locked
      - run: cargo publish --locked
        env:
          CARGO_REGISTRY_TOKEN: ${{ secrets.CARGO_REGISTRY_TOKEN }}

  build-binaries:
    strategy:
      fail-fast: false
      matrix:
        include:
          - { runner: ubuntu-latest,    target: x86_64-unknown-linux-gnu,  asset: muntjac-x86_64-unknown-linux-gnu.tar.gz }
          - { runner: ubuntu-24.04-arm, target: aarch64-unknown-linux-gnu, asset: muntjac-aarch64-unknown-linux-gnu.tar.gz }
          - { runner: macos-latest,     target: aarch64-apple-darwin,      asset: muntjac-aarch64-apple-darwin.tar.gz }
    runs-on: ${{ matrix.runner }}
    steps:
      - uses: actions/checkout@v4
        with: { submodules: recursive }
      - uses: dtolnay/rust-toolchain@stable
        with: { targets: ${{ matrix.target }} }
      - uses: Swatinem/rust-cache@v2
      - run: cargo build --release --locked --target ${{ matrix.target }}
      - run: |
          mkdir -p dist
          tar -czf "dist/${{ matrix.asset }}" -C "target/${{ matrix.target }}/release" muntjac
      - uses: actions/upload-artifact@v4
        with:
          name: ${{ matrix.asset }}
          path: dist/${{ matrix.asset }}

  github-release:
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
      - name: extract CHANGELOG section
        run: |
          tag="${GITHUB_REF_NAME#v}"
          awk -v ver="$tag" '
            /^## \[/ { if (found) exit; if ($0 ~ "\\[" ver "\\]") { found=1; next } }
            found { print }
          ' CHANGELOG.md > release-notes.md
      - run: gh release create "$GITHUB_REF_NAME" dist/* --title "$GITHUB_REF_NAME" --notes-file release-notes.md
        env:
          GH_TOKEN: ${{ secrets.GITHUB_TOKEN }}
```

Notes:
- **Job ordering:** `publish-crate` → `build-binaries` → `github-release`. The Release page is created only if both crates.io publish and all three binary builds succeeded. Avoids the "crate on crates.io but no GH Release" or vice-versa half-state.
- **Cross-compile targets:** linux-x86_64, linux-aarch64, macos-aarch64. All three are native builds on their respective runners — no cross-toolchain setup needed.
- **Tarball format:** `muntjac-<target-triple>.tar.gz` containing just the `muntjac` binary at the root. Install path: `tar -xzf <asset> && mv muntjac /usr/local/bin/`.
- **`CARGO_REGISTRY_TOKEN`:** repo secret, configured manually by the maintainer via crates.io `Account → API tokens` and stored under repo `Settings → Secrets → Actions`. **Manual prereq** — checked in the tag-cut flow (§5.5 step 3), not auto-verified.
- **`GITHUB_TOKEN`:** auto-injected. `permissions: contents: write` is required for `gh release create`.

### 5.4 `muntjac init` template update

`src/cli/init.rs:106-108` — replace the existing comment:

```rust
// Current
"# When a community registry exists, set to \"github.com/<owner>/muntjac-fixups\"\n\
 # and run `muntjac fixups update` to pin a SHA. For air-gapped or\n\
 # pre-launch usage, use a local checkout: registry = \"file:///abs/path\".\n"

// New
"# Pin a community fixup registry (recommended for native deps). Example:\n\
 #   registry = \"github.com/rsJames-ttrpg/muntjac-fixups\"\n\
 # Then run `muntjac fixups update` to fetch and pin the latest SHA.\n\
 # For local checkout / offline use: registry = \"file:///abs/path\".\n"
```

The default value of `registry` stays `"none"` — don't auto-opt users into a remote registry; surface the option in the comment.

Tests in `src/cli/init.rs::tests` (insta snapshots of the generated template) need re-baselining via `cargo insta accept`.

### 5.5 Tag-cut flow (manual maintainer checklist)

Not a script — lives in this spec as the canonical procedure. Run when all S8a CI is green and the maintainer is ready to launch.

1. Confirm `main` is green (`ci.yml`, `publish-check.yml`, `demo.yml` all passing).
2. Verify `Cargo.toml.version == "0.1.0"`, `CHANGELOG.md` has a `## [0.1.0]` block dated 2026-05-25 (or current date), repository URL is `rsJames-ttrpg/muntjac`.
3. Confirm `CARGO_REGISTRY_TOKEN` is set in `Settings → Secrets → Actions` (visible to repo maintainers as a placeholder; can't read the value back, but its presence is enough).
4. `cargo publish --dry-run --locked` locally as belt-and-suspenders (publish-check.yml has already run on the merge commit; this is a final visual check).
5. `git tag -a v0.1.0 -m "muntjac v0.1.0"` && `git push origin v0.1.0`.
6. Watch `release.yml` run. If `publish-crate` fails: **do not retag** — bump to `0.1.1` and try again (cargo versions are immutable on crates.io).
7. After the GH Release page exists, smoke-test `cargo install muntjac --version 0.1.0` from a fresh machine or container.
8. Update `muntjac-fixups/.github/workflows/ci.yml` to swap `cargo install --git ... --tag s7b-complete` for `cargo install muntjac --version 0.1.0 --locked`. Push as a small commit; tag `muntjac-fixups` as `seed-v0.1.0-published` (or just leave the existing `seed-v0.1.0` tag and bump on next change).
9. Press send on the launch post (§6).

---

## 6. Launch post stub

New file `docs/launch-post.md` with three audience-tuned drafts:

### 6.1 Short form (Buck2 Discord, Twitter/X — under 280 chars where applicable)

> muntjac v0.1.0: translate `uv.lock` into Buck2 build rules. numpy/pandas/fastapi/requests/ruff working end-to-end on Linux x86_64 + macOS arm64. Community fixup registry for native deps at github.com/rsJames-ttrpg/muntjac-fixups.
>
> `cargo install muntjac` — README has the 60-second demo. Feedback welcome.

### 6.2 Medium form (Reddit r/Python, lobste.rs)

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

### 6.3 Long form (HN Show HN)

Hand-written per the moment. Use the medium-form bullets as outline. HN audience cares about engineering choices: why Rust, why a separate fixup repo, what was hard. **Don't post until after at least one external user has confirmed the demo works on a clean machine** — HN front-page traffic on a broken README is brutal.

---

## 7. Risks and mitigations

| Risk | Mitigation |
|------|------------|
| `CARGO_REGISTRY_TOKEN` missing at tag-push time → `cargo publish` step fails, no GH Release | Tag-cut checklist puts token configuration **before** tagging (step 3). Recovery: configure token, retag as `v0.1.1` (cargo versions are immutable). |
| `cargo publish --dry-run` passes locally but fails in CI due to dirty working tree / untracked files | `--locked` flag + publish-check.yml gating PRs catches drift early. |
| Cross-compile fails on `macos-latest` (arm64) due to a C-dep assuming x86 | All deps (`gix`, `reqwest+rustls`, `tar`, `flate2`) are pure-Rust or have arm64-tested C deps. Existing ci.yml already runs `cargo test --locked` on macos-latest. If release-mode-only failure surfaces, ship without that target in v0.1.0 and add in v0.1.1. |
| README demo flakes on PyPI / `uv` version drift | Pin `uv` to a known version in `demo.yml` (existing `ci.yml` does this). PyPI's reliability is generally fine; a 503 during CI is acceptable noise. |
| `crates.io/muntjac` name is already taken | Maintainer runs `cargo publish --dry-run` against live crates.io **days before tag-cut** to confirm name availability. If taken, pick an alternate (e.g., `muntjac-buckify`) and update README + Cargo.toml. |
| `gh release create` hits upload-size limit | GH Releases allow 2 GiB per asset; muntjac binary is ~5 MiB stripped. Non-issue. |
| Demo CI takes too long (real PyPI install of numpy+pandas+requests) | uv's resolver is fast; wheels are pre-built on PyPI. Expected ~15s install + ~5s buckify. Tolerable. |

---

## 8. Plan touch list

**Create:**
- `README.md` (full rewrite, replaces the 12-line stub)
- `LICENSE`
- `CHANGELOG.md`
- `docs/launch-post.md`
- `.github/workflows/release.yml`
- `.github/workflows/publish-check.yml`
- `.github/workflows/demo.yml`

**Modify:**
- `Cargo.toml` — `[package]` table per §3.1
- `src/cli/init.rs` — template text + URL per §5.4
- `src/cli/init.rs::tests` — insta snapshot re-baseline

**Update:**
- `docs/superpowers/specs/2026-05-20-muntjac-roadmap.md` — mark S8a ✅ shipped with commit count + tag at stage close
- `docs/superpowers/TECH_DEBT.md` — close any S8a-attributable items at stage close (none expected; the three S8a TD entries are deliberate deferrals)

**Test re-baselines:**
- `cargo insta accept` for init template snapshot changes (one or two snapshots)

---

## 9. References

- Roadmap row for S8a: [`2026-05-20-muntjac-roadmap.md`](./2026-05-20-muntjac-roadmap.md) §Phase 1 → S8a
- S8b spec §1.3 (out of scope items now in scope here) + §11 (preview of S8a deliverables): [`2026-05-24-muntjac-s8b-fixups-seed-design.md`](./2026-05-24-muntjac-s8b-fixups-seed-design.md)
- Tech debt entries created by this stage's brainstorm: `TD-S8a-01`, `TD-S8a-02`, `TD-S8a-03` in [`../TECH_DEBT.md`](../TECH_DEBT.md)
- Existing CI workflow (referenced for bootstrap reuse): [`.github/workflows/ci.yml`](../../../.github/workflows/ci.yml)
- muntjac-fixups seed repo (referenced from README): <https://github.com/rsJames-ttrpg/muntjac-fixups>
