#!/usr/bin/env bash
set -euo pipefail
root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"
source linux/core-env.sh
out="${NEGS_PACKAGE_OUT:-$root/target/linux-package/$(git rev-parse --short HEAD)}"
python3 linux/release.py prepare --out "$out"
version=$(python3 -c 'import tomllib; print(tomllib.load(open("Cargo.toml", "rb"))["workspace"]["package"]["version"])')
source_archive="$out/assets/negs-$version.tar.gz"
openblas_prefix=/usr
if [ -f "$root/target/linux-toolchain/sysroot/usr/lib/libopenblas.so.0" ]; then
    openblas_prefix="$root/target/linux-toolchain/sysroot/usr"
fi
if [ "${1:-}" = --bundle ]; then
    bash linux/build.sh
    stage="$out/bundle"
    python3 linux/stage.py --destdir "$stage" --prefix / --source-archive "$source_archive" --bundle-openblas "$openblas_prefix"
    tar --sort=name --mtime="@$(git show -s --format=%ct HEAD)" --owner=0 --group=0 --numeric-owner \
        -C "$stage" -czf "$out/assets/negs-$version-linux-x86_64.tar.gz" .
    printf 'Bundle: %s\n' "$out/assets/negs-$version-linux-x86_64.tar.gz"
else
    # Standard makepkg dependency checking is retained. An already configured
    # development sysroot may use makepkg --nodeps explicitly for local testing.
    cd "$out/build"
    makepkg --force "${@}"
    cd "$root"
    python3 linux/release.py finish --out "$out" --package "$out/build/negs-$version-1-x86_64.pkg.tar.zst" --openblas-prefix "$openblas_prefix"
fi
