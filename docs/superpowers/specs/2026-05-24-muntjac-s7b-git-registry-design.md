# Muntjac S7b — Git Fetch + Cache + `fixups update` (Design)

> **Stage:** S7b (sub-stage of S7 per `2026-05-20-muntjac-roadmap.md`).
> **Status:** Design.
> **Prerequisite reading:** `2026-05-20-muntjac-design.md` §7 (fixup schema & the moat);
> `2026-05-24-muntjac-s7a-community-layering-design.md` (community layering foundation,
> which leaves `RegistryConfig::Git { url, rev }` declared but stub-erroring).

---

## 1. Scope & non-goals

S7a delivered community-layering with `registry = "none"` and `registry =
"file://<abs>"` modes. S7b completes the moat by adding the network-and-cache
layer that turns `registry = "github.com/<owner>/<repo>"` into a usable mode:

- `gix`-based fetch of a registry repo at a specific SHA (shallow, single-branch).
- Content-addressed cache at `~/.cache/muntjac/fixups/<sha>/` — full
  working-tree checkout per SHA.
- `muntjac fixups update [--rev <rev>]` — fetches HEAD of the registry's
  default branch (or `--rev`), updates `registry_rev` in `muntjac.toml`
  via `toml_edit`, prints a structured diff vs. the previous pin.
- Global `--offline` flag: refuses to fetch; errors `OfflineButCacheMiss`
  on miss; required by CI configurations that assert hermeticity.
- `file://<abs>.git` URL form (parser extension): bare-repo fetch path
  used by the test fixture; production users use `github.com/<owner>/<repo>`.

After S7b, S8 (launch polish) is the next stage.

### 1.1 In scope for S7b

| Concern | Resolution |
|---|---|
| `gix` dependency | added; uses default features (https + ssh transports) |
| `toml_edit` dependency | added; for surgical `registry_rev` writeback |
| Content-addressed cache | `<cache_root>/fixups/<sha>/` per SHA; full working-tree |
| `MUNTJAC_CACHE_HOME` env override | for test isolation |
| Cache hit detection | full 40-char hex SHA + `packages/` subdir presence |
| `fetch_into_cache(url, rev, offline)` | core fetch function, returns `FetchResult { sha, working_tree }` |
| `RegistryConfig::Git` arm in `EffectiveFixups::load` | wires fetch → load_community |
| `muntjac fixups update` subcommand | default fetches HEAD of main; `--rev` opts into specific |
| Surgical TOML writeback | via `toml_edit::Document` to preserve comments/formatting |
| Diff computation `diff_fixup_sets` | structural compare; emits `+`/`-`/`~` `DiffLine` enum |
| Global `--offline` flag | on `Globals`; threaded into `EffectiveFixups::load` |
| `OfflineButCacheMiss` error | locked message; surfaces at fetch time |
| New `file://<abs>.git` URL form | parser extension; bare-repo test path |
| `09-git-registry/` fixture | bare repo generated at test setup time; 2 snapshot tests |

### 1.2 Out of scope (deferred to post-launch / S8)

- **Symlink-traversal hardening in `extract_tarball`** (S5 TD): mis-targeted to S7b in the S7a spec. S7b does not touch tarball extraction (gix has its own internal extraction). Stays as an open S5 TD item.
- **Cache GC / pruning of stale SHAs:** the cache grows monotonically as users update their pin. v1 leaves it to the user (or to `rm -rf ~/.cache/muntjac/`). Post-launch addition: `muntjac fixups gc --keep-pinned`.
- **`muntjac fixups status`:** reports cache state, currently-pinned SHA, available updates. Useful but YAGNI for v1 — `muntjac fixups update` already shows the diff.
- **Multiple registries:** v1 supports one registry per `muntjac.toml`. Multi-registry composition is a v2 conversation.
- **SSH-key authentication for private registries:** gix supports it but v1 assumes the registry is a public GitHub repo. Private use cases work via cached fetch (run `muntjac fixups update` somewhere with credentials, ship the cache); explicit support is post-launch.
- **`fixups update --json`:** structured diff output as JSON. v1 emits human-readable text; the `DiffLine` enum is internal but JSON output is a small post-launch addition if a consumer materializes.

### 1.3 Schema-only / forward-compat

| Field / config | Status in S7b |
|---|---|
| `RegistryConfig::Git { url, rev }` | fully implemented (S7a declared, S7b implements) |
| `registry_rev` in `[fixups]` | now meaningful for Git registries; warning removed from `Config::from_raw` for that case (was added in S7a) |

---

## 2. Module layout & pipeline integration

### 2.1 Module layout delta

```
src/cache.rs                    # NEW. Resolves cache root.
                                #   - $MUNTJAC_CACHE_HOME (highest precedence)
                                #   - dirs::cache_dir()/muntjac
                                #     (dirs already honors $XDG_CACHE_HOME)

src/fixup/registry.rs           # EXTEND.
                                #   + pub fn fetch_into_cache(url, rev, offline)
                                #     -> Result<FetchResult, FixupError>
                                #   + pub struct FetchResult { sha, working_tree }
                                #   + parse_registry_config extended for
                                #     file://<abs>.git → Git form (test path)

src/fixup/layer.rs              # MODIFY.
                                #   EffectiveFixups::load gains `offline: bool`
                                #   parameter.  Git arm calls fetch_into_cache
                                #   then load_community.

src/fixup/diff.rs               # NEW.
                                #   + pub enum DiffLine { Added, Removed, Modified }
                                #   + pub fn diff_fixup_sets(old, new) -> Vec<DiffLine>
                                #   + pub fn render_diff(&[DiffLine]) -> String

src/fixup/error.rs              # EXTEND.
                                #   + GitFetch { url, rev, source }
                                #   + CacheCorrupt { path, reason }

src/error.rs                    # EXTEND.
                                #   + OfflineButCacheMiss { pin: String }
                                #   + CacheError (typed enum)

src/cli/mod.rs                  # MODIFY.
                                #   Globals { ..., offline: bool }

src/cli/buckify.rs              # MODIFY.
                                #   Pass globals.offline through to
                                #   EffectiveFixups::load.

src/cli/fixups.rs               # EXTEND.
                                #   + FixupsOp::Update { rev: Option<String> }
                                #   + fn update(rev, globals) -> Result<()>
                                #   show: pass globals.offline through too

Cargo.toml                      # ADD deps:
                                #   gix = "0.83"     (default features = https+ssh)
                                #   toml_edit = "0.22"

tests/fixtures/buck/
└── 09-git-registry/            # NEW. Bare repo built at test setup time.
    ├── muntjac.toml            # placeholders for registry URL + rev
    ├── pyproject.toml
    ├── uv.lock                 # one synthetic package: pkg-a@1.0.0
    ├── registry-source/        # raw source — NOT a git repo
    │   └── packages/pkg-a/fixups.toml
    ├── third-party/python/
    │   ├── fixups/pkg-a/fixups.toml   # local layer
    │   └── prebake/{.manifest.toml,*.whl}
    ├── expected/{BUCK,muntjac.bzl,wiring.bzl}
    └── .gitignore              # allow-list pattern per TD-S7a-01

tests/common/git_fixture.rs     # NEW (or inline). Helper:
                                #   init_bare_repo_from_source(src_dir, dest_bare)
                                #       -> Result<String, gix::init::Error>
                                # Returns the commit SHA.
```

### 2.2 Pipeline (extends S7a's threading)

```
cli/buckify.rs:
  1-5. (unchanged from S7a)
  6. EffectiveFixups::load(&config.fixups.registry,
                            &third_party_dir,
                            config.fixups.allow_local_overrides,
                            globals.offline)         # NEW arg
       For RegistryConfig::Git { url, rev }:
         resolved = fetch_into_cache(url, rev, offline)?
         load_community(&resolved.working_tree)?
  ... (rest unchanged)

cli/fixups.rs update flow:
  1. Read muntjac.toml; reject if registry is None/FileUrl with hint message
  2. Read prior registry_rev (Option<String>) for diff baseline
  3. fetch_into_cache(url, rev_or_None, globals.offline) → FetchResult
  4. If prior rev was set AND its cache dir exists:
       prev_set = load_community(<cache_root>/fixups/<prior_rev>/)
       new_set  = load_community(&result.working_tree)
       diff     = diff_fixup_sets(&prev_set, &new_set)
       print render_diff(&diff)
     Else (first-time pin):
       print "Initial pin (no prior rev to diff against)"
  5. Surgically rewrite muntjac.toml via toml_edit:
       [fixups]
       registry_rev = "<result.sha>"
     Preserves all other formatting and comments.
  6. Print footer: "Pinned <url> @ <result.sha>"
```

---

## 3. Public API surface

### 3.1 `src/cache.rs`

```rust
/// Resolves the muntjac cache root.
///
/// Precedence (highest first):
///   - $MUNTJAC_CACHE_HOME       (test isolation; also for users with
///                                non-XDG-friendly env)
///   - dirs::cache_dir() / "muntjac"  (XDG-compliant)
///
/// Creates the directory if missing.
pub fn cache_root() -> Result<PathBuf, crate::error::CacheError>;

/// Convenience: `<cache_root>/fixups/<sha>/`. Does NOT create the
/// SHA directory itself (the caller decides creation order).
pub fn fixup_cache_path_for_sha(sha: &str) -> Result<PathBuf, CacheError>;
```

### 3.2 `src/fixup/registry.rs` extensions

```rust
/// Resolved fetch result.
pub struct FetchResult {
    pub sha: String,             // full 40-char hex of resolved commit
    pub working_tree: PathBuf,   // absolute path: <cache_root>/fixups/<sha>/
}

/// Resolve `rev` to a concrete SHA and fetch into the cache.
///
/// - `rev = Some(rev_str)`:
///     - If `rev_str` is a 40-char hex SHA matching a cache hit, return immediately.
///     - Else fetch the named ref/SHA/tag/branch from the remote.
/// - `rev = None`: fetch HEAD of the remote's default branch.
/// - `offline = true`: never touch the network; error `FixupError::Offline`
///   if the cache doesn't already contain the resolved SHA.
///
/// Errors:
///   - `FixupError::GitFetch { url, rev, source }` on gix-side failures
///   - `FixupError::CacheCorrupt { path, reason }` if a cache directory exists but
///     is missing `packages/` (indicates aborted prior fetch or manual corruption)
///   - `FixupError::Offline { pin }` when offline=true and cache miss (see §7)
pub fn fetch_into_cache(
    url: &str,
    rev: Option<&str>,
    offline: bool,
) -> Result<FetchResult, crate::fixup::FixupError>;

// parse_registry_config (existing in S7a) is extended to recognize
// `file://<abs>.git` as `RegistryConfig::Git { url: "file://<abs>.git", rev }`.
// The discriminator vs. the S7a `FileUrl` form is the trailing `.git`.
```

**`file://*.git` URL semantics:** the parser passes the full URL string into
`RegistryConfig::Git { url, .. }` — gix natively understands `file://`
git protocol. No transport-level work in muntjac. Used only by the test
fixture; production users use `github.com/<owner>/<repo>` (which the
parser keeps unchanged).

### 3.3 `src/fixup/layer.rs::EffectiveFixups::load` signature change

```rust
pub fn load(
    registry: &crate::fixup::RegistryConfig,
    third_party_dir: &Path,
    allow_local_overrides: bool,
    offline: bool,                          // NEW in S7b
) -> Result<Self, FixupError> {
    let community = match registry {
        RegistryConfig::None       => FixupSet::default(),
        RegistryConfig::FileUrl(p) => load_community(p)?,
        RegistryConfig::Git { url, rev } => {
            let resolved = fetch_into_cache(url, rev.as_deref(), offline)?;
            load_community(&resolved.working_tree)?
        }
    };
    let local = if allow_local_overrides {
        load_local(third_party_dir)?
    } else {
        FixupSet::default()
    };
    Ok(Self { community, local })
}
```

**Caller migration:** ~10 call sites add the new arg. Most pass `false`
(tests, fixups-show smoke tests). `cli/buckify.rs` and `cli/fixups.rs`
pass `globals.offline`. The S7a `GitRegistryNotImplemented` error variant
becomes unused — left in place as a future fallback or removed (decision in §7).

### 3.4 `src/fixup/diff.rs`

```rust
/// One line of structured diff output between two FixupSets.
#[derive(Debug, PartialEq, Eq)]
pub enum DiffLine {
    /// Package present in `new`, absent in `old`.
    Added(pep508_rs::PackageName),
    /// Package present in `old`, absent in `new`.
    Removed(pep508_rs::PackageName),
    /// Package present in both with different content. Field names
    /// (e.g. "extra_deps", "exclude_wheels") identify what changed.
    Modified(pep508_rs::PackageName, Vec<&'static str>),
}

/// Compare two FixupSets. Returns DiffLines in canonical package-name order.
pub fn diff_fixup_sets(old: &FixupSet, new: &FixupSet) -> Vec<DiffLine>;

/// Render diff lines as the user-facing text format. Each line:
///   + <pkg>                    (added)
///   - <pkg>                    (removed)
///   ~ <pkg> (<fields>)         (modified; fields comma-joined)
pub fn render_diff(lines: &[DiffLine]) -> String;
```

**Field detection in `Modified`:** structural compare of `FixupConfig`
fields (`top.extra_deps`, `top.omit_deps`, `top.overlay`, …,
`replace_community`, `cfg_sections`). Any field that differs contributes
its `&'static str` name. The `cfg_sections` Vec is compared
as-a-whole (any difference → `"cfg_sections"` in the field list); finer
section-level diffing is post-launch.

### 3.5 `src/cli/mod.rs` Globals extension

```rust
#[derive(Args, Debug, Clone)]
pub struct Globals {
    // ... existing fields ...

    /// Refuse network operations. Use cache only.
    #[arg(long, global = true, default_value_t = false)]
    pub offline: bool,
}
```

`clap` global = true so subcommands inherit. Default `false` matches user expectation.

### 3.6 `src/cli/fixups.rs` Update subcommand

```rust
#[derive(Subcommand, Debug)]
pub enum FixupsOp {
    Show { package: String },
    Update {
        /// SHA, branch, or tag to fetch. Default: HEAD of default branch.
        #[arg(long)]
        rev: Option<String>,
    },
}
```

Update handler (sketch in §2.2). Sole writer of `registry_rev` in
`muntjac.toml` — uses `toml_edit::Document` to preserve user comments
and key ordering.

---

## 4. Cache layout & semantics

```
<cache_root>/                         # ~/.cache/muntjac (XDG) or $MUNTJAC_CACHE_HOME
└── fixups/
    ├── .staging/
    │   └── <random-uuid>/            # in-progress fetches; renamed atomically
    ├── <sha1>/                       # full working-tree checkout at <sha1>
    │   ├── packages/<pkg>/fixups.toml
    │   ├── README.md                 # whatever else is in the repo
    │   └── .git/                     # gix's clone artifact (we don't read it after checkout)
    └── <sha2>/
        └── ...
```

**Cache hit detection** (in `fetch_into_cache` step 2):
- `rev` must be a 40-char hex SHA (else we don't know what to look up).
- `<cache_root>/fixups/<rev>/` must be a directory.
- `<cache_root>/fixups/<rev>/packages/` must exist (sentinel for "checkout completed").

If the SHA dir exists but `packages/` doesn't, the cache entry is corrupt
(prior fetch crashed mid-checkout). Return `FixupError::CacheCorrupt`
with a hint to delete the path manually. v1 doesn't auto-recover —
recovery is risky (what if the user committed something?), and the
failure mode is rare enough that surfacing it loudly is the safer call.

**Atomicity:**
- Stage in `<cache_root>/fixups/.staging/<uuid>/`.
- `std::fs::rename` to `<cache_root>/fixups/<sha>/`. POSIX guarantees this
  is atomic *for the rename itself*; the user may briefly see a partial
  state if they `ls` during the rename, but no concurrent `fetch_into_cache`
  caller will see a half-populated SHA dir.
- If another process raced and the destination already exists, `rename`
  fails (cross-FS) or succeeds-with-merge (same FS, depending on kernel).
  Handle both: drop the staged copy, use the pre-existing path.

**No cache GC in v1.** Each new SHA pinned adds a new directory; old
SHAs are never removed automatically. Users with disk pressure can
`rm -rf ~/.cache/muntjac/fixups/` and re-fetch the current pin.

---

## 5. `muntjac fixups update` UX

### 5.1 Default invocation

```
$ muntjac fixups update
~ pillow (extra_deps)
+ olefile
- urllib3-legacy

Pinned github.com/<owner>/muntjac-fixups @ d3e4f56789abc...
```

Behavior:
1. Reads `muntjac.toml` from `globals.workdir()`.
2. If `[fixups] registry` is `none` or `file://...` (non-`.git`), errors:
   ```
   error: muntjac fixups update requires a git-based registry; current
          registry is `<form>`. Use `registry = "github.com/<owner>/<repo>"`.
   ```
3. Reads prior `registry_rev` (may be `None`).
4. `fetch_into_cache(url, None, globals.offline)` → `FetchResult`.
   - With `--offline`, expected to error `OfflineButCacheMiss` since we
     don't know the rev upfront (HEAD of main isn't pinned). Error is
     unambiguous — update requires network.
5. If prior rev was set and cache hit, load both `FixupSet`s and
   `render_diff` to stdout. Else: skip diff with `"Initial pin (no prior rev to diff against)"`.
6. Surgical TOML writeback via `toml_edit::Document`:
   - Open `muntjac.toml`.
   - `doc["fixups"]["registry_rev"] = result.sha` (creates key if missing).
   - Write back. Preserves all other content byte-identical except for
     the one key.
7. Print footer.

### 5.2 `--rev` invocation

```
$ muntjac fixups update --rev v2.3.0
+ scipy
~ numpy (extra_deps, cfg_sections)

Pinned github.com/<owner>/muntjac-fixups @ 0fa8b2c... (was: d3e4f56...)
```

Same flow, but step 4 passes `Some("v2.3.0")` to `fetch_into_cache`.
Step 7 footer additionally shows the prior pin for clarity.

### 5.3 Error cases

| Condition | Error |
|---|---|
| `registry = "none"` | `update requires a git-based registry; current is "none"` |
| `registry = "file:///abs/path"` (directory form) | `update requires a git-based registry; current is file:// directory form` |
| `--offline` + no cached SHA matching prior pin | `OfflineButCacheMiss { pin: <prior> }` (from fetch) |
| Network failure | `GitFetch { url, rev, source }` |
| Cache dir exists but corrupt | `CacheCorrupt { path, reason }` |
| `muntjac.toml` missing `[fixups]` table | (write path creates it; not an error) |
| `--rev <invalid>` | `GitFetch` from gix's ref-resolution failure |

---

## 6. Diff format & semantics

### 6.1 `DiffLine` shape

```rust
pub enum DiffLine {
    Added(PackageName),                       // new fixup, was absent
    Removed(PackageName),                     // fixup gone, was present
    Modified(PackageName, Vec<&'static str>), // changed; field names list what
}
```

### 6.2 Field detection in `Modified`

`diff_fixup_sets` iterates the union of package names from both `FixupSet`s.
For packages in both, structural compare `FixupConfig` fields:

| Field path | Detected as |
|---|---|
| `top.extra_deps` (Vec change) | `"extra_deps"` |
| `top.omit_deps` (Vec change) | `"omit_deps"` |
| `top.replace_deps` (Map change) | `"replace_deps"` |
| `top.prefer_wheel` (Option change) | `"prefer_wheel"` |
| `top.exclude_wheels` (Vec change) | `"exclude_wheels"` |
| `top.overlay` (Option change) | `"overlay"` |
| `top.entry_points` (Option change) | `"entry_points"` |
| `top.visibility` (Option change) | `"visibility"` |
| `top.labels` (Vec change) | `"labels"` |
| `top.runtime_env` (Map change) | `"runtime_env"` |
| `top.sdist` (Option change) | `"sdist"` |
| `replace_community` (bool change) | `"replace_community"` |
| `cfg_sections` (Vec change) | `"cfg_sections"` (any difference; no per-section diffing) |

Order of field names in the output is the order above. Empty `Vec`
→ no `Modified` entry (same as `Identical`).

### 6.3 `render_diff` output

Each `DiffLine` becomes one line:

```
+ <pkg>
- <pkg>
~ <pkg> (<field1>, <field2>)
```

Empty input → empty string (no trailing newline). Non-empty input ends
with `\n`. Lines sorted by package name (already canonical from
`diff_fixup_sets`).

---

## 7. Error handling

### 7.1 New variants

**`src/fixup/error.rs`:**

```rust
#[error("git fetch failed for {url}{}: {source}", match rev {
    Some(r) => format!(" @ {r}"),
    None => String::new(),
})]
GitFetch {
    url: String,
    rev: Option<String>,
    #[source]
    source: Box<gix::clone::fetch::Error>,
},

#[error("cache entry at {path} appears corrupt: {reason}\n  delete it and re-run `muntjac fixups update`")]
CacheCorrupt {
    path: PathBuf,
    reason: String,
},
```

**Added to `FixupError` (in `src/fixup/error.rs`):**

```rust
#[error("offline mode but cache miss for pinned rev `{pin}`\n  run `muntjac fixups update` (without --offline) first")]
Offline { pin: String },
```

**`src/error.rs` — new typed enum:**

```rust
#[derive(Debug, thiserror::Error)]
pub enum CacheError {
    #[error("could not resolve cache root: {0}")]
    CacheRootResolve(String),

    #[error("could not create cache directory {path}: {source}")]
    CreateDir {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}
```

No new `ConfigError` variant: the offline-cache-miss case is fundamentally a
fixup-fetch concern, not a config-parse concern. Centralizing in `FixupError`
keeps the error surface clean.

**Error wording locked byte-for-byte** per project convention.

### 7.2 `FixupError::Offline` emission site

`fetch_into_cache` returns `Err(FixupError::Offline { pin })` when
`offline=true` and the resolved-or-requested SHA is not in the cache.
The `pin` field carries either the explicit `rev` argument (e.g. an
SHA or tag the caller asked for) or the literal string `"(default branch)"`
when `rev` was `None`. The CLI layer (cli/buckify.rs, cli/fixups.rs)
propagates the error to anyhow, which renders the locked message.

### 7.3 `GitRegistryNotImplemented` removal

S7a's `FixupError::GitRegistryNotImplemented { registry }` becomes
unreachable once `EffectiveFixups::load`'s Git arm calls `fetch_into_cache`.
**Remove the variant** in S7b; rely on the compiler to flag any stale call
sites (there should be none beyond the S7a test that asserts on the variant).

The S7a test `effective_fixups_load_git_errors_in_s7a` becomes obsolete.
Replace it with a positive test: `effective_fixups_load_git_fetches_from_bare_repo`.

---

## 8. Testing strategy

### 8.1 Test inventory

| Layer | Test type | Location | Count |
|---|---|---|---|
| `cache_root()` precedence | unit | `src/cache.rs::tests` | 4 |
| `fetch_into_cache` cache hit | unit | `src/fixup/registry.rs::tests` | 1 |
| `fetch_into_cache` against bare repo | integration | `src/fixup/registry.rs::tests` | 4 |
| `fetch_into_cache` offline + miss | unit | `src/fixup/registry.rs::tests` | 1 |
| `fetch_into_cache` corrupt cache dir | unit | `src/fixup/registry.rs::tests` | 1 |
| `parse_registry_config` extended `file://*.git` form | unit | `src/fixup/registry.rs::tests` | 2 |
| `diff_fixup_sets` per-shape | unit | `src/fixup/diff.rs::tests` | 6 |
| `render_diff` formatting | unit | `src/fixup/diff.rs::tests` | 2 |
| `EffectiveFixups::load` git arm | unit | `src/fixup/layer.rs::tests` | 2 |
| `Globals.offline` clap parse | unit | `src/cli/mod.rs::tests` | 1 |
| `update` happy path | integration | `tests/fixups_update_smoke.rs` (NEW) | 3 |
| `update` writes muntjac.toml via toml_edit | integration | `tests/fixups_update_smoke.rs` | 2 |
| `update` rejects None/FileUrl registries | integration | `tests/fixups_update_smoke.rs` | 2 |
| Fixture 09 — git-registry end-to-end | snapshot | `tests/buckify.rs::fixture_09_git_registry_golden` | 1 |
| Fixture 09 — buckify --offline after pre-warm | snapshot | `tests/buckify.rs::fixture_09_offline_cache_hit` | 1 |

**Total new tests:** ~32. Test count after S7b: 316 → ~348.

### 8.2 Bare-repo helper

`tests/common/git_fixture.rs` (or top-of-test-file if the project has no
shared `tests/common/` module):

```rust
use gix::ObjectId;

/// Initialize a bare git repo at `bare_dest` and seed it with a single
/// commit containing every file under `source_dir`. Returns the commit SHA.
///
/// Author/committer are deterministic: ("muntjac-test", "test@example.com",
/// timestamp 0). This guarantees the SHA is stable across runs given the
/// same source content — useful for test assertions and for the
/// content-addressed cache.
pub fn init_bare_repo_from_source(
    source_dir: &std::path::Path,
    bare_dest: &std::path::Path,
) -> Result<String, Box<dyn std::error::Error>> {
    // 1. gix::init_bare(bare_dest)
    // 2. Open a temporary worktree, copy source files in
    // 3. gix::Repository::commit_as("muntjac-test", "test@example.com",
    //                                "initial", tree_id, parents=[])
    // 4. Update HEAD to point at the commit
    // 5. Return commit_id.to_hex().to_string()
}
```

**Determinism rationale:** with fixed author/email/time/message and fixed
source content, the SHA is reproducible. This means the snapshot test's
`expected/BUCK` can encode the SHA (via the `registry_rev` value used
during generation), and the test's `init_bare_repo_from_source` re-derives
the same SHA at runtime.

### 8.3 Fixture 09 — generated bare repo

`tests/fixtures/buck/09-git-registry/`:

```
muntjac.toml:
    [fixups]
    registry = "file:///REPLACED_AT_TEST_TIME/registry.git"
    registry_rev = "REPLACED_AT_TEST_TIME"      # set to the deterministic
                                                 # SHA at test setup
    allow_local_overrides = true

registry-source/                     # NOT a git repo; bare repo built from this
└── packages/
    └── pkg-a/
        └── fixups.toml              # extra_deps = ["//community:base"]

third-party/python/
├── fixups/pkg-a/fixups.toml         # extra_deps = ["//local:base"]
└── prebake/
    ├── .manifest.toml               # entry for pkg-a@1.0.0
    ├── .gitignore                   # ALLOW-LIST pattern per TD-S7a-01:
    │                                #   *
    │                                #   !.gitignore
    │                                #   !.manifest.toml
    │                                #   !*.whl
    │                                #   !*.tar.gz
    └── pkg-a-1.0.0-py3-none-any.whl

expected/{BUCK, muntjac.bzl, wiring.bzl}
```

Test flow:

```rust
#[test]
fn fixture_09_git_registry_golden() {
    let fixture_src = ...;
    let tmp = TempDir::new().unwrap();
    copy_fixture(&fixture_src, tmp.path());

    let bare_path = tmp.path().join("registry.git");
    let sha = init_bare_repo_from_source(
        &tmp.path().join("registry-source"),
        &bare_path,
    ).unwrap();

    // Substitute in muntjac.toml
    let muntjac_toml = std::fs::read_to_string(tmp.path().join("muntjac.toml"))?
        .replace("/REPLACED_AT_TEST_TIME/registry.git",
                 &format!("{}", bare_path.display()))
        .replace("registry_rev = \"REPLACED_AT_TEST_TIME\"",
                 &format!("registry_rev = \"{sha}\""));
    std::fs::write(tmp.path().join("muntjac.toml"), muntjac_toml)?;

    // Cache isolation
    let cache_home = tmp.path().join("cache");
    std::fs::create_dir_all(&cache_home).unwrap();

    let status = Command::new(env!("CARGO_BIN_EXE_muntjac"))
        .env("MUNTJAC_CACHE_HOME", &cache_home)
        .args(["-C", tmp.path().to_str().unwrap(), "buckify"])
        .status()
        .unwrap();
    assert!(status.success());

    let generated = std::fs::read_to_string(tmp.path().join("third-party/python/BUCK"))?;
    let expected = std::fs::read_to_string(fixture_src.join("expected/BUCK"))?;
    assert_eq!(generated, expected);

    // Sanity: cache was populated
    assert!(cache_home.join("fixups").join(&sha).join("packages").is_dir());
}
```

The second test `fixture_09_offline_cache_hit` runs the same flow,
then re-runs with `--offline` and asserts both success AND that no
re-fetch occurred (verifiable by removing the bare repo between
runs — if `--offline` errors, the test catches it; if it succeeds,
the cache was used).

---

## 9. Demo (exit criteria for S7b)

```bash
# In a fresh workspace using muntjac:
muntjac init  # writes starter muntjac.toml with `registry = "none"`

# Edit muntjac.toml to point at the registry:
[fixups]
registry = "github.com/<owner>/muntjac-fixups"
allow_local_overrides = true

# Fetch and pin:
muntjac fixups update
# → + numpy
# → + pillow
# → ...
# → Pinned github.com/<owner>/muntjac-fixups @ d3e4f56...

# Subsequent buckify uses cache:
muntjac buckify

# CI / hermetic build:
muntjac --offline buckify  # OK (cache populated)

# Refresh:
muntjac fixups update
# → ~ pillow (extra_deps)
# → + olefile
# → Pinned github.com/<owner>/muntjac-fixups @ 89abcde... (was: d3e4f56...)
```

---

## 10. Tech-debt items folded / re-targeted

| Item | Action in S7b |
|---|---|
| S5 symlink-traversal in `extract_tarball` | **Stays as S5/post-launch TD.** S7b doesn't touch tarball extraction; gix has its own internal extraction. The S7a spec mis-targeted this to S7b. |
| `GitRegistryNotImplemented` variant | Removed in S7b — unreachable once fetch is implemented. |
| `registry_rev` stderr warning for None/FileUrl | Stays; still meaningful (the field is parsed-but-ignored when registry isn't Git). |
| TD-S7a-01: `prebake/.gitignore = "*"` brittle | **Addressed in S7b's fixture 09.** New fixture uses the allow-list pattern (`*`, `!.gitignore`, `!.manifest.toml`, `!*.whl`, `!*.tar.gz`) — also documents the convention for S8 launch fixtures. The existing 06/07/08 fixtures stay as-is (they work; not worth churning). |

---

## 11. Open questions & risks

- **gix MSRV bump:** gix 0.83 needs rust 1.82; we're on 1.85. Comfortable margin. Watch for future gix releases — if a future MSRV exceeds ours, decide between bumping rust-version or pinning gix at an older series. Document the gix version pin in the spec for traceability.

- **Cross-platform `rename` semantics:** `std::fs::rename` on Linux/macOS handles atomic same-FS rename. On Windows (not a v1 target) it would fail for cross-FS rename. Not a concern for v0.1.0 — Windows is a non-goal.

- **Cache pollution between users on shared systems:** `~/.cache/muntjac` is per-user. Shared systems (containers, CI multi-tenancy) work via `MUNTJAC_CACHE_HOME` override.

- **Concurrent `fetch_into_cache` from two processes:** `rename` after staging handles the simple race. If two processes start fetching the same SHA simultaneously, both fetch (network waste, fine), the second `rename` fails (or succeeds-merging) — handled by checking destination existence and dropping the staged copy. No locking primitives needed.

- **`registry_rev` written without trailing newline:** `toml_edit` preserves the existing file's trailing-newline policy. If the user's `muntjac.toml` lacked a trailing newline, our writeback also lacks one. Either is acceptable; consistency with the input is the goal.

- **`fixups update` with stale prior cache entry:** if the prior `registry_rev` SHA's cache dir was manually deleted by the user, step 5 skips the diff. Footer notes "no prior cache; diff unavailable" instead of "Initial pin." Small UX nicety.

- **Multiple bare repos at the same URL with different SHAs:** the cache is content-addressed by SHA, not by URL. Two registries that share a SHA (hypothetically) would share the cache dir. Doesn't happen in practice — git SHA collisions are vanishingly unlikely.

- **Default-branch detection:** gix's `clone` with no explicit ref + `with_remote_name("origin")` resolves HEAD via the remote's symbolic-ref. Works for both `main` and `master` defaults. Tested in fixture 09.

---

## 12. What S8 will add (preview, not committed)

- README + 60-second demo CI fixture (cargo install → muntjac init → uv add numpy → muntjac vendor → muntjac buckify).
- `muntjac-fixups` repo seeded with 8 packages (lxml, pillow, cryptography, psycopg2, pyzmq, opencv-python, scipy, torch).
- Cargo metadata for publish.
- Release workflow.
- v0.1.0 tag + cargo publish.
- Launch post drafted.
