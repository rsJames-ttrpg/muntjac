#!/usr/bin/env bash
# Pre-commit gate for muntjac: cargo fmt --check + cargo clippy.
# Wired in .claude/settings.json as a PreToolUse hook scoped to `Bash(git commit *)`.
# Mirrors what CI runs — failing here means CI would fail too.
#
# Reads PreToolUse JSON on stdin (unused; the `if` filter handles scoping).
# Exits 0 on pass; outputs PreToolUse-deny JSON on fail.

set -uo pipefail

fmt_out=$(cargo fmt --check -- --color=never 2>&1)
fmt_status=$?

clippy_out=$(cargo clippy --all-targets --color=never --quiet -- -D warnings 2>&1)
clippy_status=$?

if [ "$fmt_status" -eq 0 ] && [ "$clippy_status" -eq 0 ]; then
  exit 0
fi

reason="Pre-commit gate failed (matches CI gates; failing here means CI would too):"
if [ "$fmt_status" -ne 0 ]; then
  reason+="

cargo fmt --check failed. Run \`cargo fmt\` to fix.
${fmt_out}"
fi
if [ "$clippy_status" -ne 0 ]; then
  reason+="

cargo clippy --all-targets -- -D warnings failed:
${clippy_out}"
fi
reason+="

To bypass for one commit, open /hooks (Claude Code menu) and disable temporarily."

jq -n --arg r "$reason" '{
  hookSpecificOutput: {
    hookEventName: "PreToolUse",
    permissionDecision: "deny",
    permissionDecisionReason: $r
  }
}'
