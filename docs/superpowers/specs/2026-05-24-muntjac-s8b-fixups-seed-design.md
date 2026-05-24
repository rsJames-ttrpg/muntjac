# Muntjac S8b — `muntjac-fixups` Seed Repo (Design)

> **Stage:** S8b (sub-stage of S8 per `2026-05-20-muntjac-roadmap.md`).
> **Status:** Design.
> **Prerequisite reading:** `2026-05-20-muntjac-design.md` §7 (fixup schema & the moat);
> `2026-05-24-muntjac-s7a-community-layering-design.md` §5 (community layer loader & layout);
> `2026-05-24-muntjac-s7b-git-registry-design.md` (git fetch + cache).

---

## 1. Scope & non-goals

S7a + S7b shipped the **muntjac side** of the community-registry moat: layered
fixups, file://+git registry modes, content-addressed cache, `muntjac fixups
update` workflow. **S8b ships the registry itself** — the seed `muntjac-fixups`
repo containing fixups for the 5 platform-quirky packages chosen as the v0.1.0
launch surface.

S8b is decoupled from S8a (muntjac repo polish: README, demo CI fixture, Cargo
metadata, release workflow, v0.1.0 tag). S8b ships first so that S8a's README
can confidently link to a populated registry.

### 1.1 In scope for S8b

| Concern | Resolution |
|---|---|
| Repo creation at `github.com/rsJames-ttrpg/muntjac-fixups` | Public, MIT-licensed; already created (one stub README commit) |
| Local checkout for development | `/home/jackm/repos/muntjac-fixups/` (sister to muntjac repo) |
| Repo layout | `packages/<pkg>/fixups.toml` per S7a §5 layout convention |
| Five seeded fixups | `pillow`, `cryptography`, `lxml`, `pyzmq`, `psycopg2-binary` |
| README.md | What is this; how is it consumed by muntjac; pinning workflow |
| CONTRIBUTING.md | How to add a new fixup; PR review process; license note |
| LICENSE (MIT) | Matches muntjac's license |
| `.github/workflows/ci.yml` | Schema-only validation per package via `muntjac fixups show` |
| `tests/schema-smoke/` | Synthetic pyproject + uv.lock + muntjac.toml used by CI |
| Tag `seed-v0.1.0` on launch | Marks the state shipped with muntjac v0.1.0 |

### 1.2 Out of scope (deferred to post-launch)

- **buckify + buck2 build validation per seeded package** (TD-S8b-01).
- **`torch` fixup** (TD-S8b-02) — user explicitly trimmed this from launch scope.
- **`opencv-python`, `scipy`** (TD-S8b-03) — in the original roadmap's 8-package list; trimmed to 5 for launch.
- **Auto-bump on package version updates** — manual updates for v0.1.x.
- **Formal CoC document** — a thin pointer in CONTRIBUTING.md is enough for launch.
- **GitHub Discussions / Issue templates** — defer until the first community PR surfaces a need.

### 1.3 Out of scope (S8a, not S8b)

| Concern | Where it lands |
|---|---|
| README updates in muntjac repo | S8a |
| `Cargo.toml.repository` update (jackmpcollins → rsJames-ttrpg) | S8a |
| `muntjac init` template URL update to point at seed repo | S8a |
| `cargo publish` to crates.io | S8a |
| v0.1.0 tag on muntjac | S8a |
| Release workflow | S8a |

---

## 2. Repo layout

```
muntjac-fixups/                              # github.com/rsJames-ttrpg/muntjac-fixups
├── README.md                                # what this is; how muntjac consumes it
├── CONTRIBUTING.md                          # add-a-fixup workflow; review process
├── LICENSE                                  # MIT (matches muntjac)
├── .github/
│   └── workflows/
│       └── ci.yml                           # schema-only per-package validation
├── packages/
│   ├── pillow/fixups.toml
│   ├── cryptography/fixups.toml
│   ├── lxml/fixups.toml
│   ├── pyzmq/fixups.toml
│   └── psycopg2-binary/fixups.toml
└── tests/
    └── schema-smoke/
        ├── pyproject.toml                   # depends on all 5 seeded packages
        ├── uv.lock                          # frozen lock (re-lock on dep updates)
        └── muntjac.toml                     # registry placeholder for CI substitution
```

**Directory naming:** PEP 503 normalized — lowercase, hyphens (not underscores). `psycopg2-binary` not `psycopg2_binary`.

**`packages/` layout convention:** matches what `EffectiveFixups::load(RegistryConfig::Git { url, .. })` expects — `<registry-root>/packages/<pkg>/fixups.toml` per `src/fixup/loader.rs::load_community` and the S7a design spec §5.1.

---

## 3. Seeded fixup content

Each fixup encodes the conventional shape of using that package in a Buck monorepo. Fixups reference hypothetical `//third-party/c:<lib>` targets — the user-monorepo convention. Consumers without such targets get a Buck-build-time error pointing at the missing rule; they can add the rule (real fix) or set `replace_community = true` locally (escape hatch).

### 3.1 `packages/pillow/fixups.toml`

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

### 3.2 `packages/cryptography/fixups.toml`

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

### 3.3 `packages/lxml/fixups.toml`

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

### 3.4 `packages/pyzmq/fixups.toml`

```toml
# pyzmq links against libzmq. Most wheels bundle it, but the convention
# is to provide //third-party/c:libzmq so the monorepo has one
# authoritative version.
extra_deps = [
    "//third-party/c:libzmq",
]
labels = ["bundles-native"]
```

### 3.5 `packages/psycopg2-binary/fixups.toml`

```toml
# psycopg2-binary ships its own libpq inside the wheel — no system
# dependency required. This fixup exists primarily as a registry
# signal: "use the -binary variant, not vanilla psycopg2".
labels = ["bundles-libpq", "binary-only"]
```

### 3.6 Conventions encoded by the seed

**`labels` strings:**

- `"bundles-native"` — the wheel bundles its native library(s) by default; fixup's `extra_deps` are for users who prefer system-library dynamic linking.
- `"bundles-libpq"` — specifically signals "this package ships libpq inside the wheel."
- `"binary-only"` — recommends the user pin to the -binary variant over the source variant.

These are not load-bearing for muntjac itself (the emitter passes them through to Buck's `labels = [...]` attr). They're a registry-side communication channel for downstream tooling.

**Why `//third-party/c:<lib>` and not concrete paths:**

The seed registry intentionally targets the reindeer-style convention where monorepos have a `//third-party/c/` directory with hand-written Buck rules for system libraries. This pushes the responsibility for the C-library wiring onto the user (who knows their monorepo's conventions) while still giving them a working baseline.

**Why no `cfg()` sections (except cryptography):**

Keep the seed simple. Per-platform / per-Python conditionals can be added as real-world usage surfaces concrete needs. The cryptography pre-42 cfg section exists because the version split is well-known and the bundle behavior changed cleanly.

---

## 4. README.md (canonical)

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

---

## 5. CONTRIBUTING.md (canonical)

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

---

## 6. CI (`.github/workflows/ci.yml`)

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

### 6.1 `tests/schema-smoke/muntjac.toml`

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

### 6.2 `tests/schema-smoke/pyproject.toml`

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

### 6.3 `tests/schema-smoke/uv.lock`

Generated once via `uv lock` against `pyproject.toml` and committed. Re-lock on dependency updates (rare in v0.1.x).

`uv` itself isn't required to RUN the CI — the lockfile is committed. `uv` is only needed when refreshing the lockfile.

**Note on `file://` registry pointing at "." with `third_party_dir = "."`:** the muntjac.toml's `third_party_dir = "."` points at `tests/schema-smoke/` — there's no `fixups/` subdir under it, so the local layer is empty. The `file:///<repo-root>` registry path means `load_community` walks `<repo-root>/packages/` (mounting the SAME directory that lives at `../../packages/` relative to the smoke fixture). This is the seed repo dogfooding its own registry.

---

## 7. Integration with the muntjac repo

S8b lives in a separate repo, but touches muntjac in three places — **all bundled into S8a, NOT S8b:**

1. **`Cargo.toml.repository`:** currently points at `jackmpcollins/muntjac`; should point at `rsJames-ttrpg/muntjac`. **S8a.**
2. **`muntjac init` template:** the existing comment `# When a community registry exists, set to "github.com/<owner>/muntjac-fixups"` should become a concrete URL `"github.com/rsJames-ttrpg/muntjac-fixups"`. **S8a.**
3. **README:** S8a's README will reference muntjac-fixups by URL (registry name, recommended initial pin, link to CONTRIBUTING). **S8a.**

S8b adds to muntjac's `docs/superpowers/TECH_DEBT.md` (in the muntjac repo, since TECH_DEBT is the central log per [[feedback_tech_debt_log]]):

- **TD-S8b-01:** buckify + buck2 build validation per seeded package — deferred (post-launch).
- **TD-S8b-02:** `torch` fixup — deferred (user trimmed).
- **TD-S8b-03:** `opencv-python`, `scipy` fixups — deferred.

These TD entries are committed in the muntjac repo as part of S8b's "ship" commit (separate from the seed repo work).

---

## 8. Workflow & tag

**S8b development happens in two parallel repos:**

- `/home/jackm/repos/muntjac/` — design spec (this doc), TECH_DEBT additions, plan doc.
- `/home/jackm/repos/muntjac-fixups/` — actual fixup files, CI, README/CONTRIBUTING/LICENSE, smoke test.

**Plan execution order (rough):**

1. (in muntjac) Commit this design spec + S8b plan + roadmap update.
2. (in muntjac-fixups) Add LICENSE, README, CONTRIBUTING.
3. (in muntjac-fixups) Add 5 `packages/<pkg>/fixups.toml` files.
4. (in muntjac-fixups) Add `tests/schema-smoke/{pyproject,uv.lock,muntjac.toml}`.
5. (in muntjac-fixups) Add `.github/workflows/ci.yml`.
6. (in muntjac-fixups) Push to origin; verify CI green.
7. (in muntjac-fixups) Tag `seed-v0.1.0`.
8. (in muntjac) Add TD-S8b-01/02/03 entries; update roadmap S8b row to ✅ shipped; tag `s8b-complete` (on muntjac side, marking S8b's completion as a muntjac-side stage).

**Tag conventions:**
- muntjac repo: `s8b-complete` (consistent with prior stages: `s0-complete`, `s1-complete`, etc.)
- muntjac-fixups repo: `seed-v0.1.0` (matches the muntjac v0.1.0 that will reference it)

---

## 9. Exit criteria (S8b is shipped when)

- ✅ `github.com/rsJames-ttrpg/muntjac-fixups` is public with the file tree above.
- ✅ CI on the seed repo passes (schema-only validation of all 5 packages via `muntjac fixups show`).
- ✅ `tests/schema-smoke/uv.lock` is committed (not gitignored).
- ✅ Seed repo can be cloned by `cargo install --git ... --tag s7b-complete --locked` followed by `muntjac fixups show pillow` against the smoke fixture.
- ✅ Tag `seed-v0.1.0` on the seed repo marks the launch state.
- ✅ muntjac repo: design spec + plan committed; TD-S8b-01/02/03 logged in TECH_DEBT; roadmap S8b row marked ✅ shipped; tag `s8b-complete` on muntjac side.

---

## 10. Open questions & risks

- **`cargo install --git --tag s7b-complete` is fragile:** if we later force-push s7b-complete (or any tag), CI starts using a moved target. Mitigation: pin to the SHA `s7b-complete` resolved to at S8b ship time, recorded in a comment in the workflow file. Better: once S8a publishes v0.1.0, swap to `cargo install muntjac --version 0.1.0 --locked`.

- **GitHub URL in `cargo install --git` requires `--locked` to use our `Cargo.lock`:** without `--locked`, cargo would re-resolve and might pull different versions. The `--locked` flag is in the spec; the implementer should NOT drop it.

- **`uv lock` may produce different lockfiles across uv versions:** the smoke test's committed `uv.lock` is pinned to whatever uv produced when generated. If a future uv version refuses the lockfile, regenerate. Document this in CONTRIBUTING.md if it becomes a real issue.

- **`//third-party/c:libjpeg` references in seed fixups have NO real targets behind them:** users who clone muntjac-fixups and try to `muntjac buckify` against pillow will get a Buck error when building (not at fixup-load time). This is by design — the registry is data, not code — but the seed README should call this out so new users aren't surprised. **The README does:** the "How is this consumed?" section implicitly assumes a Buck monorepo with `//third-party/c/` set up; the local-overrides section explains the `replace_community = true` escape hatch.

- **Schema-smoke uses `file://` to point at the SAME repo:** the smoke registry is `file:///<repo-root>` — the seed repo's CI loads its own fixups via the file:// path. Cute and works. If the schema-smoke fixture is moved (e.g., reorganized), the relative path in `muntjac.toml` becomes wrong. Mitigation: CI script substitutes the absolute path at runtime (no hardcoded path); `muntjac.toml` has only the `file:///REPLACED_BY_CI_SCRIPT` sentinel.

- **CONTRIBUTING.md links to muntjac's design spec on `main`:** if the linked file moves (e.g., spec restructure), the link breaks. Acceptable tradeoff for v0.1.0; revisit if it becomes a churn issue.

- **No DCO / CLA:** v0.1.0 takes "by submitting a PR, you agree to MIT" as informal consent. Aligns with how most small OSS projects operate. Adopt formal DCO/CLA if a corporate contributor requires one (post-launch concern).

---

## 11. What S8a will add (preview, not committed)

- README rewrite in the muntjac repo (canonical project entry point with quickstart, reference to muntjac-fixups).
- 60-second demo wired as a CI fixture (cargo install → muntjac init → uv add numpy → muntjac vendor → muntjac buckify).
- `Cargo.toml.repository` update (`jackmpcollins` → `rsJames-ttrpg`).
- `Cargo.toml` polish (keywords, categories, authors, homepage).
- `muntjac init` template URL update to `github.com/rsJames-ttrpg/muntjac-fixups`.
- `.github/workflows/release.yml` — runs `cargo publish` on tag push.
- `cargo publish --dry-run` gate in CI.
- v0.1.0 tag + cargo publish.
- Release notes drafted (release-note paragraph for v0.1.0, includes the launch-post stub).
