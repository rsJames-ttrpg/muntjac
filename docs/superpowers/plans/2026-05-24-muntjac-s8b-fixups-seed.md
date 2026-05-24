# S8b — `muntjac-fixups` Seed Repo Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Populate the `muntjac-fixups` registry repo with 5 seeded fixups (pillow, cryptography, lxml, pyzmq, psycopg2-binary), schema-only CI, README/CONTRIBUTING/LICENSE — then tag `seed-v0.1.0` on the seed repo and `s8b-complete` on the muntjac repo.

**Architecture:** Work spans **two repos**. The muntjac-fixups repo (at `/home/jackm/repos/muntjac-fixups/`, origin `github.com/rsJames-ttrpg/muntjac-fixups`) gets the actual fixup TOML files, docs, and CI workflow. The muntjac repo (at `/home/jackm/repos/muntjac/`, origin `github.com/rsJames-ttrpg/muntjac`) gets the new TECH_DEBT entries + roadmap mark-shipped + `s8b-complete` tag. No Rust code changes in either repo — this stage is pure data + docs + CI.

**Tech Stack:** TOML (fixup files + smoke fixture), GitHub Actions YAML, Markdown (README/CONTRIBUTING), `uv lock` (for the smoke fixture's uv.lock). `cargo install --git ... --tag s7b-complete --locked` for installing muntjac into the seed repo's CI.

**Prerequisite reading:**
- `docs/superpowers/specs/2026-05-24-muntjac-s8b-fixups-seed-design.md` (locked design)
- `docs/superpowers/specs/2026-05-24-muntjac-s7a-community-layering-design.md` §5 (`load_community` layout convention)
- `docs/superpowers/TECH_DEBT.md` TD-S7a-01 (allow-list `.gitignore` pattern — N/A here since there's no prebake, but useful precedent for committed test fixtures)

---

## Phase 0 — Pre-flight check

The seed repo already exists at `/home/jackm/repos/muntjac-fixups/` with one commit (a 17-byte stub README) and origin `https://github.com/rsJames-ttrpg/muntjac-fixups.git`. The muntjac repo's design spec + roadmap update are already committed (commit `7e07398`). No setup task needed.

### Implementer working-directory convention

**Every task below specifies which repo it operates in.** Use the absolute path for clarity:

- **muntjac-fixups repo:** `/home/jackm/repos/muntjac-fixups/`
- **muntjac repo:** `/home/jackm/repos/muntjac/`

For git operations inside an implementer subagent, prefer `cd /home/jackm/repos/<repo>/` at the top of the work or use `git -C /home/jackm/repos/<repo>/ <subcommand>`. The current shell's `cwd` shouldn't be assumed.

---

## Phase 1 — License + docs (muntjac-fixups)

### Task 1: Add LICENSE + finalize README + CONTRIBUTING

**Repo:** `muntjac-fixups` at `/home/jackm/repos/muntjac-fixups/`

**Files:**
- Create: `LICENSE` (MIT)
- Modify: `README.md` (replace 17-byte stub with the canonical README from spec §4)
- Create: `CONTRIBUTING.md` (canonical contents from spec §5)

- [ ] **Step 1: Add MIT LICENSE**

Create `/home/jackm/repos/muntjac-fixups/LICENSE`:

```
MIT License

Copyright (c) 2026 muntjac-fixups contributors

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

- [ ] **Step 2: Replace the stub README**

Overwrite `/home/jackm/repos/muntjac-fixups/README.md` with the canonical version from the design spec §4:

```markdown
# muntjac-fixups

The community fixup registry for [muntjac](https://github.com/rsJames-ttrpg/muntjac) —
a translator from `uv.lock` to [Buck2](https://buck2.build/) build rules.

A "fixup" is a small TOML document that tells muntjac how a specific
Python package should be wired up in Buck. It covers things muntjac
can't derive from `uv.lock` alone: system-library dependencies,
overlay files, visibility, and so on.

This repo currently seeds **5 packages**:

- pillow, cryptography, lxml, pyzmq, psycopg2-binary

See [packages/](./packages/) for the fixup TOML for each.

## How is this consumed?

In your `muntjac.toml`:

    [fixups]
    registry = "github.com/rsJames-ttrpg/muntjac-fixups"
    registry_rev = "<sha-from-`muntjac fixups update`>"
    allow_local_overrides = true

Then run `muntjac fixups update` to pin a specific revision and fetch
the fixups into your cache.

## Local overrides

Every package has an escape hatch: drop a `fixups.toml` at
`<third_party_dir>/fixups/<pkg>/fixups.toml` to override or extend
the community fixup. Set `replace_community = true` to disable the
community fixup entirely for that package.

## License

MIT — see [LICENSE](./LICENSE).
```

- [ ] **Step 3: Create CONTRIBUTING.md**

Create `/home/jackm/repos/muntjac-fixups/CONTRIBUTING.md` with the canonical contents from spec §5:

```markdown
# Contributing to muntjac-fixups

Thanks for your interest. This registry seeds the most common Python
packages that benefit from explicit Buck wiring. Additions are very
welcome.

## Adding a new fixup

1. Create `packages/<package-name>/fixups.toml`. Use the PEP 503
   normalized form for the directory name (lowercase, hyphens not
   underscores).
2. Write the fixup body. See [muntjac's design spec][fixup-schema]
   for the full schema.
3. Open a PR. CI will validate the schema.

[fixup-schema]: https://github.com/rsJames-ttrpg/muntjac/blob/main/docs/superpowers/specs/2026-05-20-muntjac-design.md

## Testing locally

    # Clone this repo
    git clone https://github.com/rsJames-ttrpg/muntjac-fixups
    cd muntjac-fixups/tests/schema-smoke

    # Install muntjac (post-v0.1.0)
    cargo install muntjac --locked

    # Validate
    muntjac fixups show pillow

## PR review

A maintainer eyeballs the TOML for sensibility, confirms the
`//third-party/c:<lib>` targets reference real system libraries
(not arbitrary user paths), and merges.

## License

By submitting a PR, you agree to license your contribution under MIT.
```

- [ ] **Step 4: Commit (in muntjac-fixups repo)**

```bash
cd /home/jackm/repos/muntjac-fixups
git add LICENSE README.md CONTRIBUTING.md
git commit -m "docs: add LICENSE + canonical README + CONTRIBUTING"
```

---

## Phase 2 — Seeded fixups (muntjac-fixups)

### Task 2: Add the 5 seeded fixup TOML files

**Repo:** `muntjac-fixups` at `/home/jackm/repos/muntjac-fixups/`

**Files:**
- Create: `packages/pillow/fixups.toml`
- Create: `packages/cryptography/fixups.toml`
- Create: `packages/lxml/fixups.toml`
- Create: `packages/pyzmq/fixups.toml`
- Create: `packages/psycopg2-binary/fixups.toml`

All TOML contents are byte-for-byte from the design spec §3. PEP 503 normalized directory names (lowercase, hyphens).

- [ ] **Step 1: Create the 5 directories + files**

```bash
cd /home/jackm/repos/muntjac-fixups
mkdir -p packages/pillow packages/cryptography packages/lxml packages/pyzmq packages/psycopg2-binary
```

Create `packages/pillow/fixups.toml`:

```toml
# Pillow needs libjpeg and zlib at runtime for the most common features
# (JPEG/PNG decoding). Provide these as Buck targets in your monorepo,
# or set `replace_community = true` in your local fixup to opt out.
extra_deps = [
    "//third-party/c:libjpeg",
    "//third-party/c:zlib",
]
labels = ["bundles-native"]
```

Create `packages/cryptography/fixups.toml`:

```toml
# Modern cryptography wheels (>=42) bundle a static OpenSSL. The
# `extra_deps` here is for monorepos that prefer to dynamic-link against
# the system OpenSSL (smaller binary, easier CVE updates).
extra_deps = [
    "//third-party/c:openssl",
]
labels = ["bundles-native"]

# Pre-42 versions did NOT bundle OpenSSL — surface a hard requirement.
["cfg(version = \"<42\")"]
extra_deps = [
    "//third-party/c:openssl",
]
```

Create `packages/lxml/fixups.toml`:

```toml
# lxml links against libxml2 + libxslt. Manylinux wheels bundle them, but
# the conventional pattern is to provide them as Buck targets so reverse
# deps can also use libxml2 without ABI mismatch.
extra_deps = [
    "//third-party/c:libxml2",
    "//third-party/c:libxslt",
]
labels = ["bundles-native"]
```

Create `packages/pyzmq/fixups.toml`:

```toml
# pyzmq links against libzmq. Most wheels bundle it, but the convention
# is to provide //third-party/c:libzmq so the monorepo has one
# authoritative version.
extra_deps = [
    "//third-party/c:libzmq",
]
labels = ["bundles-native"]
```

Create `packages/psycopg2-binary/fixups.toml`:

```toml
# psycopg2-binary ships its own libpq inside the wheel — no system
# dependency required. This fixup exists primarily as a registry
# signal: "use the -binary variant, not vanilla psycopg2".
labels = ["bundles-libpq", "binary-only"]
```

- [ ] **Step 2: Sanity-check the TOML is parseable**

The implementer can verify the TOML parses cleanly before committing:

```bash
cd /home/jackm/repos/muntjac-fixups
for f in packages/*/fixups.toml; do
  python3 -c "import tomllib; tomllib.loads(open('$f').read()); print('OK', '$f')"
done
```

Expected: 5 lines of `OK ...`. If any FAIL, the implementer fixes the syntax before committing.

(`tomllib` is in the Python 3.11+ stdlib. If Python is older, fall back to `python3 -c "import toml; ..."` or just `cargo run --manifest-path /home/jackm/repos/muntjac/Cargo.toml -- ...` invocations — but for this simple parse check, Python's stdlib is cheapest.)

- [ ] **Step 3: Commit (in muntjac-fixups repo)**

```bash
cd /home/jackm/repos/muntjac-fixups
git add packages/
git commit -m "feat: seed 5 fixups (pillow, cryptography, lxml, pyzmq, psycopg2-binary)"
```

---

## Phase 3 — Schema-smoke fixture (muntjac-fixups)

### Task 3: Add `tests/schema-smoke/` fixture (pyproject + muntjac.toml + uv.lock)

**Repo:** `muntjac-fixups` at `/home/jackm/repos/muntjac-fixups/`

**Files:**
- Create: `tests/schema-smoke/pyproject.toml`
- Create: `tests/schema-smoke/muntjac.toml` (with `file:///REPLACED_BY_CI_SCRIPT` placeholder)
- Create: `tests/schema-smoke/uv.lock` (generated by `uv lock`)

- [ ] **Step 1: Create the directory + pyproject.toml**

```bash
cd /home/jackm/repos/muntjac-fixups
mkdir -p tests/schema-smoke
```

Create `tests/schema-smoke/pyproject.toml`:

```toml
[project]
name = "fixups-smoke"
version = "0.0.1"
requires-python = ">=3.12,<3.13"
dependencies = [
    "pillow",
    "cryptography",
    "lxml",
    "pyzmq",
    "psycopg2-binary",
]
```

- [ ] **Step 2: Create muntjac.toml with the registry placeholder**

Create `tests/schema-smoke/muntjac.toml`:

```toml
manifest_path   = "pyproject.toml"
third_party_dir = "."
python_versions = ["3.12"]

[platforms.linux-x86_64-gnu]
target    = "x86_64-unknown-linux-gnu"
manylinux = "2_17"

[fixups]
registry = "file:///REPLACED_BY_CI_SCRIPT"
allow_local_overrides = true
```

- [ ] **Step 3: Generate uv.lock**

Verify `uv` is on PATH (`which uv` — should resolve). If not, install via the official one-liner: `curl -LsSf https://astral.sh/uv/install.sh | sh`.

```bash
cd /home/jackm/repos/muntjac-fixups/tests/schema-smoke
uv lock
```

Expected: produces a `uv.lock` file resolving all 5 packages. Take whatever versions PyPI is currently serving — the lockfile freezes them.

- [ ] **Step 4: Sanity-check the lockfile exists and is non-trivial**

```bash
ls -la /home/jackm/repos/muntjac-fixups/tests/schema-smoke/uv.lock
wc -l /home/jackm/repos/muntjac-fixups/tests/schema-smoke/uv.lock
```

Expected: file exists, several hundred lines (uv lockfiles are verbose).

- [ ] **Step 5: Commit (in muntjac-fixups repo)**

```bash
cd /home/jackm/repos/muntjac-fixups
git add tests/
git commit -m "test: schema-smoke fixture for CI validation"
```

---

## Phase 4 — CI workflow + first push (muntjac-fixups)

### Task 4: Add `.github/workflows/ci.yml`

**Repo:** `muntjac-fixups` at `/home/jackm/repos/muntjac-fixups/`

**Files:**
- Create: `.github/workflows/ci.yml`

- [ ] **Step 1: Create the workflow file**

```bash
cd /home/jackm/repos/muntjac-fixups
mkdir -p .github/workflows
```

Create `.github/workflows/ci.yml`:

```yaml
name: CI
on: [push, pull_request]

jobs:
  validate-fixups:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - uses: dtolnay/rust-toolchain@stable

      # Install muntjac. Pre-v0.1.0: via git tag. Post-v0.1.0: switch to
      # `cargo install muntjac --version 0.1.0 --locked` and drop the
      # `--git`/`--tag` args.
      - name: Install muntjac
        run: |
          cargo install --git https://github.com/rsJames-ttrpg/muntjac \
            --tag s7b-complete \
            --locked

      - name: Validate each seeded package
        run: |
          # Substitute the file:// placeholder with the absolute repo path.
          REPO_ROOT="$(pwd)"
          sed -i "s|file:///REPLACED_BY_CI_SCRIPT|file://${REPO_ROOT}|" \
              tests/schema-smoke/muntjac.toml

          cd tests/schema-smoke
          for pkg in pillow cryptography lxml pyzmq psycopg2-binary; do
            echo "=== fixups show $pkg ==="
            muntjac fixups show "$pkg"
          done
```

- [ ] **Step 2: Commit (in muntjac-fixups repo)**

```bash
cd /home/jackm/repos/muntjac-fixups
git add .github/
git commit -m "ci: schema-only validation per seeded package"
```

---

### Task 5: Push muntjac-fixups to GitHub + watch CI

**Repo:** `muntjac-fixups` at `/home/jackm/repos/muntjac-fixups/`

- [ ] **Step 1: Verify all 4 prior commits are present locally**

```bash
git -C /home/jackm/repos/muntjac-fixups log --oneline
```

Expected output (most recent first):
```
<sha> ci: schema-only validation per seeded package
<sha> test: schema-smoke fixture for CI validation
<sha> feat: seed 5 fixups (pillow, cryptography, lxml, pyzmq, psycopg2-binary)
<sha> docs: add LICENSE + canonical README + CONTRIBUTING
<sha> first commit
```

(5 commits total, with the original "first commit" stub.)

- [ ] **Step 2: Push to origin**

```bash
git -C /home/jackm/repos/muntjac-fixups push origin main
```

Expected: clean push to `https://github.com/rsJames-ttrpg/muntjac-fixups.git`.

- [ ] **Step 3: Find the new CI run**

```bash
sleep 6
gh run list --repo rsJames-ttrpg/muntjac-fixups --limit 1 --json databaseId,status,workflowName,headSha
```

Expected: one in-progress run for the CI workflow on the pushed SHA.

- [ ] **Step 4: Watch CI**

```bash
RUN_ID=$(gh run list --repo rsJames-ttrpg/muntjac-fixups --limit 1 --json databaseId --jq '.[0].databaseId')
gh run watch "$RUN_ID" --repo rsJames-ttrpg/muntjac-fixups --exit-status
```

Expected: success. The CI installs muntjac via `cargo install --git ... --tag s7b-complete` (slow — first run may take 3-5 minutes for the cargo install step), then runs `muntjac fixups show` for each of the 5 packages.

- [ ] **Step 5: If CI fails, iterate**

Common failure modes and remedies:

| Failure | Remedy |
|---|---|
| `cargo install` fails — tag not found | Verify `s7b-complete` exists at `github.com/rsJames-ttrpg/muntjac`. Adjust the tag name in `.github/workflows/ci.yml` if it differs. |
| `muntjac fixups show pillow` errors with parse error | TOML syntax bug in `packages/pillow/fixups.toml`. Fix locally, commit, push. |
| `sed` substitution doesn't work | Different sed dialect on ubuntu-latest. Verify the substitution literal matches the file content exactly. |
| `cargo install` succeeds but `muntjac fixups show` says "no fixup for pkg" | The `file://` registry path resolves to a directory without the `packages/<pkg>/fixups.toml` file. Verify `pwd` in the CI step matches the repo checkout root and `packages/` is a sibling. |

Each iteration: fix locally → `git add` → `git commit` → `git push` → re-watch CI.

- [ ] **Step 6: Verify all 5 packages show output**

After CI is green, inspect the CI log to confirm each `muntjac fixups show <pkg>` block printed actual TOML (not just headers). E.g., for pillow, the output should include `extra_deps = ["//third-party/c:libjpeg", "//third-party/c:zlib"]`.

```bash
gh run view "$RUN_ID" --repo rsJames-ttrpg/muntjac-fixups --log | grep -A3 'fixups show pillow'
```

Expected: the TOML content for pillow's fixup appears in the log.

If pre-existing CI iteration introduced separate commits, that's fine. T7 ties everything off.

---

## Phase 5 — muntjac repo bookkeeping

### Task 6: Add TD-S8b-01/02/03 entries to muntjac's TECH_DEBT.md

**Repo:** `muntjac` at `/home/jackm/repos/muntjac/`

**Files:**
- Modify: `docs/superpowers/TECH_DEBT.md`

- [ ] **Step 1: Find the S7b section heading for insertion point**

```bash
grep -n '^### From S7b' /home/jackm/repos/muntjac/docs/superpowers/TECH_DEBT.md
```

Expected: shows the S7b section heading at some line N. Insert a new S8b section AFTER all S7b entries (just before the `---` separator that precedes the `## Resolved` section).

- [ ] **Step 2: Find the exact insertion point (the `---` before `## Resolved`)**

```bash
grep -n '^---$\|^## Resolved' /home/jackm/repos/muntjac/docs/superpowers/TECH_DEBT.md | tail -3
```

Note the line numbers. The new S8b section goes BEFORE the `---` that precedes `## Resolved`.

- [ ] **Step 3: Insert the new S8b section**

Insert before the `---` separator that precedes `## Resolved`:

```markdown
### From S8b final stage review (2026-05-24, pre-tag)

#### TD-S8b-01: Seed-repo CI is schema-only; doesn't validate fixups at buck2-build time
- **Source:** S8b design spec §1.2; user direction at brainstorm time.
- **Severity:** Minor (narrow validation; trusts that the schema-correct fixups produce correct Buck rules)
- **What:** The muntjac-fixups CI runs `muntjac fixups show <pkg>` for each seeded package. This catches schema errors and parse failures but does NOT validate that the fixup actually produces working Buck rules at `muntjac buckify` time, let alone that a `python_binary` using the package actually builds.
- **Why:** Real validation requires running `muntjac buckify` on a pyproject that uses each package, then `buck2 build` of a synthetic python_binary that imports it. Requires `//third-party/c:<lib>` targets to exist in the test fixture — would need to write Buck rules from scratch for libjpeg/openssl/libzmq/libxml2/libxslt. Significant work; not blocking launch narrative.
- **Fix:** Add a `tests/buck2-build/` fixture with `BUCK` files for the `//third-party/c:<lib>` targets, run `muntjac buckify` + `buck2 build` per package in CI.
- **Target:** post-v0.1.0; bundle with the first round of community PRs that surface real-world breakage.

#### TD-S8b-02: `torch` fixup deferred
- **Source:** S8b design spec §1.2; user direction at brainstorm time.
- **Severity:** Minor (post-launch addition)
- **What:** No `packages/torch/fixups.toml` in the seed. Torch is a high-profile Python package; many users will want a community fixup.
- **Why:** Torch wheels are 600MB+ (especially CUDA variants); the cuda-discrimination story is complex (`torch-cpu`, `torch-cuda10`, `torch-cuda11`, `torch-cuda12`); CI testing of torch is expensive. Best designed once we see how community contributors approach it.
- **Fix:** Add `packages/torch/fixups.toml` with cfg-based wheel discrimination + extra_native_libs for CUDA runtime. Possibly split into `torch-cpu` and `torch-cuda-*` variants.
- **Target:** post-v0.1.0.

#### TD-S8b-03: `opencv-python` and `scipy` fixups deferred
- **Source:** S8b design spec §1.2; user trimmed seed list from 8 → 5.
- **Severity:** Polish (post-launch additions)
- **What:** No `packages/opencv-python/fixups.toml` or `packages/scipy/fixups.toml` in the seed. Both are in the original roadmap's 8-package list.
- **Why:** opencv-python has `opencv-python` vs `opencv-python-headless` confusion; scipy ships clean wheels and mostly needs no fixup. Easy adds when an interested user PRs them.
- **Fix:** Add both files. opencv: clarify the headless-vs-not convention; scipy: probably empty `labels` body since wheels are typically clean.
- **Target:** post-v0.1.0 — likely first community PR.
```

- [ ] **Step 4: Commit (in muntjac repo)**

```bash
cd /home/jackm/repos/muntjac
git add docs/superpowers/TECH_DEBT.md
git commit -m "docs(s8b): log TD-S8b-01/02/03 for deferred seed-repo follow-ups"
```

---

### Task 7: Mark S8b ✅ shipped in roadmap; create both tags; push everything

**Repos:** Both `muntjac` and `muntjac-fixups`.

This is the wrap-up task. Updates the roadmap, creates the two tags (one in each repo), pushes both.

- [ ] **Step 1: Verify the muntjac-fixups push state matches expectations**

```bash
git -C /home/jackm/repos/muntjac-fixups log --oneline origin/main..HEAD
git -C /home/jackm/repos/muntjac-fixups log --oneline | head -10
```

Expected: no unpushed local commits (all pushed in Task 5); 5+ commits in the history (more if CI iteration added fixup commits).

- [ ] **Step 2: Tag `seed-v0.1.0` on muntjac-fixups**

```bash
git -C /home/jackm/repos/muntjac-fixups tag seed-v0.1.0
git -C /home/jackm/repos/muntjac-fixups push origin seed-v0.1.0
```

Expected: tag created on the latest commit (which is the green-CI commit); pushed to origin.

- [ ] **Step 3: Update roadmap in muntjac repo**

Find the S8b row in the roadmap:

```bash
grep -n 'S8b' /home/jackm/repos/muntjac/docs/superpowers/specs/2026-05-20-muntjac-roadmap.md
```

Update the table row from:
```markdown
| S8b | [2026-05-24-muntjac-s8b-fixups-seed-design.md](./2026-05-24-muntjac-s8b-fixups-seed-design.md) | (not yet written) | ⬜ next |
```

to:
```markdown
| S8b | [2026-05-24-muntjac-s8b-fixups-seed-design.md](./2026-05-24-muntjac-s8b-fixups-seed-design.md) | [2026-05-24-muntjac-s8b-fixups-seed.md](../plans/2026-05-24-muntjac-s8b-fixups-seed.md) | ✅ shipped (tag `s8b-complete`, seed repo at `seed-v0.1.0`, N commits) |
```

Replace `N` with the actual count: `git -C /home/jackm/repos/muntjac log --oneline s7b-complete..HEAD | wc -l`.

Update the S8b section heading from:
```markdown
#### S8b — `muntjac-fixups` seed repo
```

to:
```markdown
#### S8b — `muntjac-fixups` seed repo ✅ shipped
```

Append after the "Touches:" line:
```markdown
**Shipped:** N commits in muntjac repo (design spec + plan + TECH_DEBT entries + roadmap mark-shipped) + M commits in muntjac-fixups repo (LICENSE/docs + 5 fixups + smoke fixture + CI + tag). Seed repo tagged `seed-v0.1.0`. CI green on the seed repo.
```

Replace `N` and `M` with the actual commit counts:
- `N = git -C /home/jackm/repos/muntjac log --oneline s7b-complete..HEAD | wc -l`
- `M = git -C /home/jackm/repos/muntjac-fixups log --oneline | wc -l` (minus 1 for the original "first commit" stub, if you want to count only S8b-attributable commits)

- [ ] **Step 4: Commit roadmap update (in muntjac repo)**

```bash
cd /home/jackm/repos/muntjac
git add docs/superpowers/specs/2026-05-20-muntjac-roadmap.md
git commit -m "docs(s8b): mark S8b ✅ shipped in roadmap"
```

- [ ] **Step 5: Tag `s8b-complete` on muntjac and push**

```bash
git -C /home/jackm/repos/muntjac tag s8b-complete
git -C /home/jackm/repos/muntjac push origin main
git -C /home/jackm/repos/muntjac push origin s8b-complete
```

- [ ] **Step 6: Watch CI for muntjac**

The muntjac repo's push may trigger its own CI run (verify what events `ci.yml` listens to). If it does, watch it:

```bash
sleep 6
gh run list --repo rsJames-ttrpg/muntjac --branch main --limit 1 --json databaseId,status
```

If a new run appears, watch it to ensure no regression:

```bash
RUN_ID=$(gh run list --repo rsJames-ttrpg/muntjac --branch main --limit 1 --json databaseId --jq '.[0].databaseId')
gh run watch "$RUN_ID" --repo rsJames-ttrpg/muntjac --exit-status
```

Expected: green (the push only added docs, no code changes).

- [ ] **Step 7: Verify both repos are in the expected state**

```bash
# muntjac
git -C /home/jackm/repos/muntjac tag --list 's8*'
git -C /home/jackm/repos/muntjac log --oneline -3

# muntjac-fixups
git -C /home/jackm/repos/muntjac-fixups tag --list 'seed-*'
git -C /home/jackm/repos/muntjac-fixups log --oneline -3
```

Expected:
- muntjac: `s8b-complete` tag exists; HEAD is the roadmap-update commit.
- muntjac-fixups: `seed-v0.1.0` tag exists; HEAD is the CI-green commit.

S8b is shipped.

---

## Self-review

**Spec coverage check (run against `docs/superpowers/specs/2026-05-24-muntjac-s8b-fixups-seed-design.md`):**

| Spec section | Covered by |
|---|---|
| §1.1 5 seeded fixups | Task 2 |
| §1.1 README/CONTRIBUTING/LICENSE | Task 1 |
| §1.1 `.github/workflows/ci.yml` | Task 4 |
| §1.1 `tests/schema-smoke/` fixture | Task 3 |
| §1.1 `seed-v0.1.0` tag on seed repo | Task 7 |
| §2 repo layout | Tasks 1-4 |
| §3 per-package fixup content | Task 2 (verbatim from spec) |
| §4 README canonical content | Task 1 (verbatim from spec) |
| §5 CONTRIBUTING canonical content | Task 1 (verbatim from spec) |
| §6 CI workflow | Task 4 (verbatim from spec) |
| §6.1 schema-smoke muntjac.toml | Task 3 |
| §6.2 schema-smoke pyproject.toml | Task 3 |
| §6.3 schema-smoke uv.lock | Task 3 |
| §7 integration with muntjac (TECH_DEBT entries) | Task 6 |
| §8 workflow & tag (both tags) | Task 7 |
| §9 exit criteria | Tasks 5 (CI green) + 7 (tags pushed) |

**Placeholder scan:** No "TBD"/"fill in"/"similar to" patterns; every step has concrete content. The CI-iteration table in Task 5 Step 5 lists common failure modes with specific remedies (not "debug and fix").

**Type/path consistency:**
- `/home/jackm/repos/muntjac-fixups/` referenced consistently (vs. `muntjac-fixups/` relative)
- `/home/jackm/repos/muntjac/` referenced consistently
- `s7b-complete` tag name for `cargo install --git --tag` is consistent with the actual muntjac tag
- `seed-v0.1.0` and `s8b-complete` tag names match the spec §8

All cross-references resolve.

---

## Execution

Plan complete and saved to `docs/superpowers/plans/2026-05-24-muntjac-s8b-fixups-seed.md`.

Per the established cadence ([[feedback_planning_cadence]]), execute via `superpowers:subagent-driven-development` — fresh subagent per task, two-stage review between tasks.

**Cross-repo note for the executor:** every dispatched implementer subagent should be given the explicit repo path in its prompt. The implementer subagents do NOT inherit the controller's CWD — assume the implementer needs `cd /home/jackm/repos/<repo>` at the top of its work.
