#!/usr/bin/env bash
# Give a new worktree the main checkout's Cargo build output as an APFS
# copy-on-write clone, so it reuses every compiled dependency instead of
# building them all again. Takes seconds and no extra disk until files change;
# Cargo still rebuilds the workspace crates, whose paths differ.
#
# Runs from .git/hooks/post-checkout (install with --install). Git passes the
# previous HEAD, which is all zeros only when `git worktree add` (or a clone)
# checks the tree out for the first time.
set -euo pipefail

if [[ "${1:-}" == --install ]]; then
    hooks="$(git rev-parse --git-common-dir)/hooks"
    # The main checkout's copy: new worktrees often branch from an older
    # commit (agents start from origin/main) that may not have this script.
    printf '#!/bin/sh\n# Seed new worktrees with the main build output (scripts/seed_worktree_build.sh).\nscript="$(cd "$(git rev-parse --git-common-dir)/.." && pwd)/scripts/seed_worktree_build.sh"\n[ -x "$script" ] && "$script" "$@"\nexit 0\n' > "$hooks/post-checkout"
    chmod +x "$hooks/post-checkout"
    echo "Installed $hooks/post-checkout"
    exit 0
fi

[[ "${1:-}" =~ ^0+$ ]] || exit 0
here="$(git rev-parse --show-toplevel)"
main="$(cd "$(git rev-parse --git-common-dir)/.." && pwd)"
[[ "$here" != "$main" ]] || exit 0

mkdir -p "$here/target"
for profile in release debug; do
    src="$main/target/$profile"
    dst="$here/target/$profile"
    [[ -d "$src" && ! -e "$dst" ]] || continue
    # -c clones (APFS); fall back silently to nothing on other filesystems.
    if cp -c -R "$src" "$dst" 2>/dev/null; then
        # Incremental caches are tied to the main checkout's source paths.
        rm -rf "$dst/incremental"
        echo "seeded target/$profile from the main checkout" >&2
    else
        rm -rf "$dst"
    fi
done
