#!/bin/bash
#
# Copyright (c) 2026 Emilio Navarrete Lineros <enavarre@outlook.com>
#
# Permission to use, copy, modify, and distribute this software for any
# purpose with or without fee is hereby granted, provided that the above
# copyright notice and this permission notice appear in all copies.
#
# THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
# WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
# MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR
# ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
# WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
# ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
# OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
#
# PreToolUse hook (Bash): before a `git commit`, run `cargo xtask lz check`
# and `cargo fmt --all -- --check` in the checkout the command runs in (the
# main checkout or an agent's worktree). Exit 2 blocks the commit and hands
# the failure to the model; anything that is not a commit passes at once.
# Clippy and the smokes stay in `just ci` (too slow for every commit).
payload=$(cat)
cmd=$(printf '%s' "$payload" | jq -r '.tool_input.command // empty')
printf '%s\n' "$cmd" | grep -qE '(^|[;&|(]|&&)[[:space:]]*git([[:space:]]+-C[[:space:]]+[^[:space:]]+)?[[:space:]]+commit([[:space:]]|$)' || exit 0
dir=$(printf '%s' "$payload" | jq -r '.cwd // empty')
cd "${dir:-${CLAUDE_PROJECT_DIR:-.}}" 2>/dev/null && cd "$(git rev-parse --show-toplevel 2>/dev/null)" 2>/dev/null || exit 0
[ -f lineage.toml ] || exit 0
export PATH=/opt/homebrew/opt/rustup/bin:$PATH
if ! out=$(cargo xtask lz check 2>&1); then
    printf 'commit blocked by .claude/hooks/pre-commit-check.sh: `cargo xtask lz check` failed in %s\n%s\n' "$PWD" "$(printf '%s\n' "$out" | grep -v '^ *\(Compiling\|Finished\|Running\)' | tail -30)" >&2
    exit 2
fi
if ! out=$(cargo fmt --all -- --check 2>&1); then
    printf 'commit blocked by .claude/hooks/pre-commit-check.sh: `cargo fmt --all -- --check` failed in %s (run `cargo fmt --all`)\n%s\n' "$PWD" "$(printf '%s\n' "$out" | head -40)" >&2
    exit 2
fi
exit 0
