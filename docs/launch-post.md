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
