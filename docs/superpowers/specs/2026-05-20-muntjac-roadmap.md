# Muntjac Roadmap

**Status:** draft v1 (2026-05-20)
**Companion to:** `2026-05-20-muntjac-design.md`
**Purpose:** Decompose the full design into ordered, demoable stages. Each stage gets its own spec and implementation plan when its turn comes.

---

## Conventions

- Each stage produces a **demoable, testable artifact**, not just internal scaffolding.
- Each stage's design spec lives at `docs/superpowers/specs/YYYY-MM-DD-muntjac-s<n>-<topic>-design.md`.
- Each stage's implementation plan lives at `docs/superpowers/plans/YYYY-MM-DD-muntjac-s<n>-<topic>-plan.md`.
- **Specs are written stage-by-stage**, not all upfront. Later specs incorporate what was learned from earlier stages.
- A stage is "done" when its exit criteria pass in CI and the demo works end-to-end.
- Follow-ups discovered during a stage's implementation are filed as separate follow-up specs (not retro-edited into the original).

---

## Phase 1 — v0.1.0 launch path

The credible-launch surface: numpy, pandas, fastapi, requests, ruff working end-to-end on Linux x86_64 + macOS arm64. Local fixups + community registry layering active.

### S0 — Scaffolding & config

**Scope:** CLI skeleton (clap), `muntjac init`, `muntjac.toml` parser with rich errors, error type machinery, GitHub Actions CI on three platforms.

**Exit criteria:**
- `muntjac init` writes a starter `muntjac.toml` + `.gitignore` snippet, refuses to overwrite existing files without `--force`.
- `muntjac --help` lists all phase-1 verbs (even ones not yet implemented, marked as unimplemented).
- Malformed `muntjac.toml` produces an error message naming the bad field and line.
- CI green on `ubuntu-latest`, `ubuntu-24.04-arm`, `macos-latest`.

**Demo:** From an empty repo, `muntjac init` produces a working starter config that `muntjac --check-config` accepts.

**Touches:** `src/main.rs`, `src/cli/`, `src/config.rs`, `src/error.rs`, `.github/workflows/`.

---

### S1 — uv.lock parser & dep graph

**Scope:** Parse `uv.lock` into typed structures. Construct the dep graph. Evaluate env markers per `(platform, python_version)`. Drop dev-only deps unless opted in.

**Exit criteria:**
- A hidden `muntjac debug print-deps` command reads `uv.lock` + `muntjac.toml`, prints the resolved dep graph as JSON (one entry per `(package, version, platform, python_version)` tuple).
- Fixtures cover: dev-deps, optional-deps (`[project.optional-dependencies]`), env-marker conditionals (e.g. `typing_extensions; python_version < "3.11"`), multiple versions of the same package, cycles (rejected with a clear error).
- env marker evaluation cross-checked against `pep508_rs` reference behavior in unit tests.

**Demo:** On a fixture project with 30+ deps including env-marker conditionals, `muntjac debug print-deps` produces a stable JSON snapshot.

**Touches:** `src/lock/`, `src/platform.rs` (markers).

---

### S2 — Platform model & wheel selector

**Scope:** Platform definitions (manylinux, musllinux, macOS, with version baselines). PEP 425 tag parsing and scoring. PEP 600 manylinux aliasing. PEP 656 musllinux. The wheel selector: given a package's wheel list and a `(platform, python_version)`, pick the best match or report "no wheel."

**Exit criteria:**
- A hidden `muntjac debug pick-wheels` command takes `uv.lock` + `muntjac.toml`, prints a table `(pkg, ver, platform, py_ver) → wheel URL | NO_WHEEL`.
- Unit tests cover: PEP 425 scoring (`cp3X-cp3X-manylinux_2_17_x86_64` > `cp3X-abi3-manylinux_2_17_x86_64` > `py3-none-any`), manylinux2014 ↔ manylinux_2_17 aliasing, musllinux 1.1 ↔ 1.2 ordering, macOS deployment-target ordering.

**Demo:** On the numpy 2.1.3 wheel list, every cell in the (platform × python_version) matrix correctly resolves to the right wheel.

**Touches:** `src/platform.rs`, `src/wheel/`.

---

### S3 — First BUCK emitter (pure-python, single platform)

**Scope:** Deterministic BUCK formatter. The emitter: walks the resolved dep graph, writes `BUCK`, `muntjac.bzl`, `PACKAGE`, `config/BUCK`. Snapshot tests via `insta`. Determinism test (run twice, byte-compare).

**Exit criteria:**
- `muntjac buckify` on the `01-pure-python` fixture produces byte-identical artifacts matching golden files.
- Running buckify twice in a row produces no diff (`10-determinism` fixture).
- Sorted iteration order documented and tested: `(package_name, version, platform, python_version)` lexicographic.

**Demo:** Fixture project with a handful of pure-python deps buckifies; the BUCK is human-readable and matches what's in the design spec §6.

**Touches:** `src/buck/`, `tests/fixtures/01-pure-python/`, `tests/fixtures/10-determinism/`.

---

### S4 — Multi-platform + multi-python BUCK ("numpy day one")

**Scope:** Extend the emitter to write per-(platform × python_version) `prebuilt_python_library` variants and the alias-with-select pattern. End-to-end smoke test that actually runs `buck2 build`.

**Exit criteria:**
- `02-numpy-pandas` fixture buckifies.
- CI on Linux x86_64 runs `buck2 build //third-party/python:numpy` against the generated output and succeeds.
- CI on macOS arm64 runs the same and succeeds.
- `03-musllinux` fixture exercises a wheel that has only `*musllinux*` tags.

**Demo:** A real `import numpy as np; print(np.zeros(3))` Python binary built by Buck against muntjac-generated rules.

**Touches:** `src/buck/emit.rs` extension, CI e2e workflow.

---

### S5 — Pure-python sdist prebake

**Scope:** Sdist classifier (allowlisted build backends + adjacent-source heuristic). `uv build` shellout for pure-python sdists. Native-sdist error path with the precise error message contracted in the design spec.

**Exit criteria:**
- A pure-python sdist in `uv.lock` is converted at `muntjac vendor` time and treated as a wheel by the emitter.
- `09-native-sdist-error` fixture asserts the error message format byte-for-byte (with `(package, version, platform, python_version)` substituted in).
- The classifier's allowlist + heuristic is documented and tested with 5+ real-world examples (small, ~1MB sdists committed to the test corpus).

**Demo:** A fixture with a pure-python sdist-only package (e.g. `urllib3` of a specific old version) buckifies and builds.

**Touches:** `src/sdist/`, `src/uv.rs`.

---

### S6 — Local fixups ✅ shipped

**Scope:** `FixupConfig` schema (serde-derived from the design spec §7). `cfg()` predicate parser + evaluator. Layering algorithm (local-only at this stage; registry comes in S7). Applied in the BUCK emitter. Overlay via http_file-mode genrule (decision recorded in the S6 spec §5.3).

**Exit criteria:**
- `05-local-fixup` fixture exercises `extra_deps`, `omit_deps`, `replace_deps`, `prefer_wheel`, `exclude_wheels`, `overlay`, explicit `entry_points`, `visibility`, `labels`, `runtime_env`, plus cfg sections (`target_os`, `all(version, python)`). `entry_points = true` parses but errors at apply per spec §1.2 ([[TD-S6-02]]).
- `cfg()` predicates work for `version`, `python`, `target_os`, `target_arch`, `target_env`, plus `all`/`any`/`not`.
- `muntjac fixups show <pkg>` prints the merged effective fixup as canonical TOML.
- Per-package fixup overlay materializes via a generated `genrule(unzip → cp → zip -qrX)` in http_file mode (vendor-mode overlay deferred). `unzip`/`zip` required on PATH at Buck-build time.

**Demo:** `tests/fixtures/buck/05-local-fixup/` overlays a marker module into `tomli==2.0.1`'s real PyPI wheel; CI runs `buck2 run //tests/smoke:overlay_demo` on ubuntu-latest and greps for `OVERLAY_APPLIED: True`. Synthetic-package coverage of the fields the real wheel can't exercise (`omit_deps`, `replace_deps`, `entry_points`, `runtime_env`) lives in `src/buck/emit.rs::tests::` unit tests.

**Touches:** `src/fixup/{schema,cfg,layer,loader,validate,overlay,error}.rs`, `src/buck/emit.rs`, `src/buck/string_writer.rs`, `src/cli/fixups.rs`, `src/cli/buckify.rs`, `tests/buckify.rs`, `tests/fixups_show_smoke.rs`, `.github/workflows/ci.yml`.

**Shipped:** 28 commits (incl. design spec + plan + 3 post-tag-attempt CI fixes for the overlay genrule: export_file wrapping, genrule toolchain, unzip cwd); plan `docs/superpowers/plans/2026-05-23-muntjac-s6-local-fixups.md`; design `docs/superpowers/specs/2026-05-23-muntjac-s6-local-fixups-design.md`. Pre-tag follow-ups logged in `docs/superpowers/TECH_DEBT.md` (TD-S6-01..06).

---

### S7 — Community fixup registry

Decomposed into two sub-stages during the S7a brainstorm (split rationale: capability boundary — layering is independently demoable without git fetch).

#### S7a — Community layering (no-network registry modes) ✅ shipped

**Scope:** `RegistryConfig` typed enum (`None`/`FileUrl`/`Git`). Community ⊕ local layering via per-layer-then-merge algorithm. `replace_community = true` escape hatch. `allow_local_overrides = false` toggle. `muntjac fixups show <pkg>` layered output. `BuildEmitContext<'a>` refactor (TD-S6-04). Air-gapped modes only: `registry = "none"`, `registry = "file://…"`. Git fetch defers to S7b.

**Exit criteria:**
- Three new fixtures (`06-community-fixup`, `07-allow-local-overrides-false`, `08-replace-community`) snapshot-test the layering algorithm.
- `EffectiveFixups::resolve` correctly produces per-cell merged fixups across layers.
- `replace_community = true` on local drops the community layer for that package.
- `allow_local_overrides = false` skips loading local fixups.
- `muntjac fixups show <pkg>` prints labeled community + local blocks.

**Demo:** Synthetic-package fixture with a vendored-style registry directory at `tests/fixtures/buck/06-community-fixup/registry/packages/<pkg>/fixups.toml` plus local fixups; snapshot diff proves correct layered output.

**Touches:** `src/fixup/{registry,layer,loader,schema,error}.rs`, `src/buck/emit.rs`, `src/cli/{buckify,fixups,init}.rs`, `src/config.rs`, `tests/fixtures/buck/{06,07,08}-*/`.

**Shipped:** 24 commits (incl. design spec + plan + 19 implementation + 2 final-cleanup + 1 post-tag CI fix for missing fixture-08 prebake stubs); plan `docs/superpowers/plans/2026-05-24-muntjac-s7a-community-layering.md`; design `docs/superpowers/specs/2026-05-24-muntjac-s7a-community-layering-design.md`. TD-S6-04 resolved in commits `b4a2267` (BuildEmitContext refactor) + `13856de` (EffectiveFixups migration).

#### S7b — Git fetch & cache ✅ shipped

**Scope:** `gix` dependency. `~/.cache/muntjac/fixups/<sha>/` content-addressed cache. `muntjac fixups update` command with structured diff output. `--offline` integration test. S5's symlink-traversal hardening (re-targeted from S5 TECH_DEBT).

**Exit criteria:**
- `muntjac fixups update` fetches the pinned registry, caches by git sha, prints a structured diff of fixup changes.
- `RegistryConfig::Git { url, rev }` mode produces correct buckify output (functionally equivalent to S7a's `file://` mode pointing at the cached checkout).
- `--offline` integration test confirms no network access when pin is cached; errors with `OfflineButCacheMiss` if pin is not cached.

**Demo:** From a fresh checkout, `muntjac fixups update` then `muntjac buckify` produces a BUCK that incorporates community fixups for the test corpus.

**Touches:** `src/fixup/registry.rs` (extends S7a), `src/cache.rs` (NEW), `src/fixup/diff.rs` (NEW), `src/cli/fixups.rs` (`update` subcommand), `src/cli/buckify.rs`, `Cargo.toml` (gix + toml_edit + dirs deps), `tests/common/git_fixture.rs` (NEW shared helper), `tests/fixtures/buck/09-git-registry/`.

**Shipped:** 16 commits (design spec + plan + 12 implementation + 1 style cleanup + post-tag-S7a TD-S7a-01 log + final TD-S7b-01/roadmap doc); plan `docs/superpowers/plans/2026-05-24-muntjac-s7b-git-registry.md`; design `docs/superpowers/specs/2026-05-24-muntjac-s7b-git-registry-design.md`. Pre-tag follow-up logged: TD-S7b-01 (raw-SHA-on-cache-miss falls through to default branch — narrow correctness gap, S8/post-launch fix).

---

### S8 — Launch polish & v0.1.0

Decomposed into two sub-stages during the S8 brainstorm (split rationale: separate repos — S8b lands in a new `muntjac-fixups` repo, S8a stays in muntjac).

#### S8b — `muntjac-fixups` seed repo ✅ shipped

**Scope:** New repo at `github.com/rsJames-ttrpg/muntjac-fixups`. 5 seeded packages (pillow, cryptography, lxml, pyzmq, psycopg2-binary; torch + opencv + scipy deferred per launch trim). README, CONTRIBUTING.md, MIT LICENSE. Schema-only CI via `muntjac fixups show`. `tests/schema-smoke/` synthetic fixture.

**Exit criteria:**
- `github.com/rsJames-ttrpg/muntjac-fixups` is public with the seeded packages + docs.
- CI on the seed repo passes (schema-only validation).
- Tag `seed-v0.1.0` on the seed repo marks the launch state.
- muntjac repo: design spec + plan committed; TD-S8b-01/02/03 logged in TECH_DEBT.

**Demo:** `muntjac fixups show pillow` (against the schema-smoke fixture) prints the seeded pillow fixup as canonical TOML.

**Touches:** `muntjac-fixups` repo (separate); `docs/superpowers/specs/2026-05-24-muntjac-s8b-fixups-seed-design.md`; `docs/superpowers/TECH_DEBT.md` (TD-S8b-01/02/03 entries).

**Shipped:** 4 commits in muntjac repo (design spec + plan + TECH_DEBT entries + roadmap mark-shipped) + 4 commits in muntjac-fixups repo (LICENSE/docs + 5 fixups + smoke fixture + CI). Seed repo tagged `seed-v0.1.0`. CI green on the seed repo.

#### S8a — muntjac repo polish & v0.1.0

**Scope:** README rewrite (canonical project entry point). 60-second demo wired as CI fixture. `Cargo.toml.repository` update (`jackmpcollins` → `rsJames-ttrpg`). Cargo metadata polish (keywords, categories, authors). `muntjac init` template URL update to reference seed repo. Release workflow. `cargo publish --dry-run` gate. v0.1.0 tag + cargo publish. Release notes drafted.

**Exit criteria:**
- README's `cargo install muntjac && muntjac init && uv add numpy && muntjac vendor && muntjac buckify` demo passes as a CI test.
- `cargo publish --dry-run` succeeds.
- v0.1.0 git tag exists; release notes drafted.
- Launch post drafted (not posted by tooling; the maintainer presses send).

**Demo:** A new user can go from `cargo install muntjac` to a working Buck-built Python binary in five minutes by following the README.

**Touches:** `README.md`, `.github/workflows/release.yml`, `Cargo.toml` metadata, `src/cli/init.rs` (template URL).

---

## Phase 2 — post-launch (v0.2.0+)

Not blocking v0.1.0; sequenced once Phase 1 has shipped and feedback is in hand.

### S9 — Vendor mode

`muntjac vendor --mode=committed` downloads wheels into `third-party/python/vendor/` and the emitter replaces `http_file(urls=…)` with relative source refs. Buck builds become air-gapped.

### S10 — Audit + unused

`muntjac audit` cross-checks `uv.lock` against the pypa/advisory-database OSV dump. `muntjac unused` reports vendored wheels not referenced by any tree (analog of reindeer's `vendor --cleanup`).

### S11 — Multi-tree

`[tree.<name>]` blocks parse, `--tree` flag scopes commands, each tree gets its own `third_party_dir`. The regime-3 escape hatch (incompatible dep universes per Python era).

---

## Stage dependency graph

```
                   ┌─── S0 (scaffolding) ───┐
                   │                        │
                   ▼                        ▼
            S1 (lockfile)              S2 (wheels)
                   │                        │
                   └───────────┬────────────┘
                               ▼
                        S3 (first BUCK)
                               │
                               ▼
                  S4 (multi-platform, "numpy day one")
                               │
                               ▼
                       S5 (sdist prebake)
                               │
                               ▼
                       S6 (local fixups)
                               │
                               ▼
                  S7a (community layering, no-network)
                               │
                               ▼
                  S7b (git fetch + cache + fixups update)
                               │
                               ▼
                  S8b (muntjac-fixups seed repo)
                               │
                               ▼
                  S8a (muntjac README + cargo publish + v0.1.0)
                               │
                               ▼
            ────────── v0.1.0 SHIPPED ──────────
                               │
              ┌────────────────┼────────────────┐
              ▼                ▼                ▼
      S9 (vendor mode)  S10 (audit/unused)  S11 (multi-tree)
```

S1 and S2 can be developed in parallel after S0; they each feed S3.

All other stages are strictly sequential — each depends on the artifact the prior stage produces.

---

## Specs index

Filled in as specs are written. Hyperlinks become real once the file exists.

| Stage | Spec | Plan | Status |
|---|---|---|---|
| Design | [2026-05-20-muntjac-design.md](./2026-05-20-muntjac-design.md) | n/a | ✅ committed |
| Roadmap | this document | n/a | ✅ committed |
| S0 | [2026-05-20-muntjac-s0-scaffolding-design.md](./2026-05-20-muntjac-s0-scaffolding-design.md) | [2026-05-20-muntjac-s0-scaffolding.md](../plans/2026-05-20-muntjac-s0-scaffolding.md) | ✅ shipped (tag `s0-complete`, 15 commits, 29 tests) |
| S1 | [2026-05-20-muntjac-s1-lockfile-design.md](./2026-05-20-muntjac-s1-lockfile-design.md) | [2026-05-20-muntjac-s1-lockfile.md](../plans/2026-05-20-muntjac-s1-lockfile.md) | ✅ shipped (tag `s1-complete`, 19 commits, 68 tests) |
| S2 | [2026-05-20-muntjac-s2-wheel-selector-design.md](./2026-05-20-muntjac-s2-wheel-selector-design.md) | [2026-05-20-muntjac-s2-wheel-selector.md](../plans/2026-05-20-muntjac-s2-wheel-selector.md) | ✅ shipped (tag `s2-complete`, 26 commits, 118 tests) |
| S3 | [2026-05-21-muntjac-s3-buck-emitter-design.md](./2026-05-21-muntjac-s3-buck-emitter-design.md) | [2026-05-21-muntjac-s3-buck-emitter.md](../plans/2026-05-21-muntjac-s3-buck-emitter.md) | ✅ shipped (tag `s3-complete`, 21 commits, 145 tests) |
| S4 | [2026-05-21-muntjac-s4-multiplatform-design.md](./2026-05-21-muntjac-s4-multiplatform-design.md) | [2026-05-21-muntjac-s4-multiplatform.md](../plans/2026-05-21-muntjac-s4-multiplatform.md) | ✅ shipped (tag `s4-complete`, 31 commits, 158 tests) |
| S5 | [2026-05-22-muntjac-s5-sdist-prebake-design.md](./2026-05-22-muntjac-s5-sdist-prebake-design.md) | [2026-05-22-muntjac-s5-sdist-prebake.md](../plans/2026-05-22-muntjac-s5-sdist-prebake.md) | ✅ shipped (tag `s5-complete`, 18 commits, 190 tests) |
| S6 | [2026-05-23-muntjac-s6-local-fixups-design.md](./2026-05-23-muntjac-s6-local-fixups-design.md) | [2026-05-23-muntjac-s6-local-fixups.md](../plans/2026-05-23-muntjac-s6-local-fixups.md) | ✅ shipped (tag `s6-complete`, 28 commits, 262 tests) |
| S7a | [2026-05-24-muntjac-s7a-community-layering-design.md](./2026-05-24-muntjac-s7a-community-layering-design.md) | [2026-05-24-muntjac-s7a-community-layering.md](../plans/2026-05-24-muntjac-s7a-community-layering.md) | ✅ shipped (tag `s7a-complete`, 24 commits, 316 tests) |
| S7b | [2026-05-24-muntjac-s7b-git-registry-design.md](./2026-05-24-muntjac-s7b-git-registry-design.md) | [2026-05-24-muntjac-s7b-git-registry.md](../plans/2026-05-24-muntjac-s7b-git-registry.md) | ✅ shipped (tag `s7b-complete`, 16 commits, 346 tests) |
| S8b | [2026-05-24-muntjac-s8b-fixups-seed-design.md](./2026-05-24-muntjac-s8b-fixups-seed-design.md) | [2026-05-24-muntjac-s8b-fixups-seed.md](../plans/2026-05-24-muntjac-s8b-fixups-seed.md) | ✅ shipped (tag `s8b-complete`, seed repo at `seed-v0.1.0`, 4 commits) |
| S8a | (not yet written) | (not yet written) | ⬜ blocked on S8b |

---

## Implementation cadence

For each stage:

1. **Brainstorm the stage's spec** (focused on that stage's slice; references the design spec for shared context).
2. **User reviews and approves the spec.**
3. **Write the implementation plan** via `superpowers:writing-plans`.
4. **User reviews and approves the plan.**
5. **Execute** via `superpowers:executing-plans` (or `subagent-driven-development` for parallelizable work).
6. **Verify** exit criteria pass; review under `superpowers:requesting-code-review`.
7. **Commit, tag stage as ✅**, and move on.

The roadmap document is updated at the end of each stage with what shipped, what slipped, and any follow-up specs filed.

**Tech debt accumulated during stage reviews is logged in [`../TECH_DEBT.md`](../TECH_DEBT.md).** Each stage's brainstorm should skim it for items targeted at the upcoming stage; the planner should fold them in as explicit task items.
