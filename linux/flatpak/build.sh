#!/usr/bin/env bash
# Build the Flatpak from this checkout and write the single-file bundle.
#   linux/flatpak/build.sh OUTPUT_DIRECTORY
# Needs flatpak and flatpak-builder; installs the pinned GNOME runtime/SDK and
# Rust extension from Flathub into the user installation when missing.
set -euo pipefail
root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"
mkdir -p "$1"
out=$(readlink -f "$1")
state=${NEGS_FLATPAK_STATE:-$root/target/flatpak}
version=$(python3 -c 'import sys, tomllib; print(tomllib.load(open(sys.argv[1], "rb"))["package"]["version"])' "$root/linux/Cargo.toml")
python3 "$root/linux/flatpak/cargo_sources.py" --check
flatpak remote-add --user --if-not-exists flathub https://dl.flathub.org/repo/flathub.flatpakrepo
flatpak-builder --user --install-deps-from=flathub --force-clean --disable-rofiles-fuse \
    --state-dir="$state/state" --repo="$state/repo" "$state/build" \
    "$root/linux/flatpak/io.github.noahhaasis.negs.json"
bundle="$out/negs-$version-linux-x86_64.flatpak"
flatpak build-bundle --runtime-repo=https://dl.flathub.org/repo/flathub.flatpakrepo \
    "$state/repo" "$bundle" io.github.noahhaasis.negs
printf 'Flatpak bundle: %s\n' "$bundle"
