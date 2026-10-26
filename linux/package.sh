#!/usr/bin/env bash
set -euo pipefail
root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"
source linux/core-env.sh
out="$root/target/linux-package"
mkdir -p "$out"
# Release archives contain the reviewed commit only. Keep private validation
# assets and untracked machine-specific handoffs out of distributable source.
if ! git diff --quiet HEAD -- linux crates docs/linux-app.md docs/linux-core-validation.md; then
    printf '%s\n' 'Commit the release source before packaging.' >&2
    exit 1
fi
git archive --format=tar --prefix=negs-0.7.0/ HEAD \
    .gitattributes Cargo.toml Cargo.lock LICENSE crates data linux scripts \
    assets/negs-icon.png assets/negs-icon.svg \
    docs/linux-app.md docs/linux-core-validation.md docs/linux-core-agent-handoff.md \
    docs/linux-ui-parity.md docs/linux-ui-validation.md | gzip -n > "$out/negs-0.7.0.tar.gz"
source_hash=$(sha256sum "$out/negs-0.7.0.tar.gz" | cut -d ' ' -f 1)
sed "s/sha256sums=('SKIP')/sha256sums=('$source_hash')/" linux/arch/PKGBUILD > "$out/PKGBUILD"
if [ "${1:-}" = --bundle ]; then
    bash linux/build.sh
    stage="$out/bundle"
    rm -rf -- "$stage"
    extra=()
    if ! pkg-config --exists openblas 2>/dev/null || [ -f "$root/target/linux-toolchain/sysroot/usr/lib/libopenblas.so.0" ]; then
        if [ -f "$root/target/linux-toolchain/sysroot/usr/lib/libopenblas.so.0" ]; then
            extra=(--bundle-openblas "$root/target/linux-toolchain/sysroot/usr")
        fi
    fi
    python3 linux/stage.py --destdir "$stage" --prefix / --source-archive "$out/negs-0.7.0.tar.gz" "${extra[@]}"
    tar --sort=name --mtime="@$(git show -s --format=%ct HEAD)" --owner=0 --group=0 --numeric-owner \
        -C "$stage" -czf "$out/negs-0.7.0-linux-x86_64.tar.gz" .
    printf 'Bundle: %s\n' "$out/negs-0.7.0-linux-x86_64.tar.gz"
else
    # Standard makepkg dependency checking is retained. An already configured
    # development sysroot may use makepkg --nodeps explicitly for local testing.
    cd "$out"
    makepkg --force "${@}"
fi
