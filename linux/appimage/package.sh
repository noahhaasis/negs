#!/usr/bin/env bash
# Build the AppImage from a release source archive in the pinned Ubuntu 24.04
# baseline container.
#   linux/appimage/package.sh SOURCE_ARCHIVE OUTPUT_DIRECTORY
set -euo pipefail
here="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd -- "$here/../.." && pwd)"
archive=$(readlink -f "$1")
mkdir -p "$2"
out=$(readlink -f "$2")
cache=${NEGS_APPIMAGE_CACHE:-$root/target/appimage/cache}
mkdir -p "$cache"
image="negs-appimage-build:$(sha256sum "$here/Dockerfile" | cut -c1-12)"
docker image inspect "$image" >/dev/null 2>&1 || docker build -t "$image" "$here"
docker run --rm \
    -e SOURCE_DATE_EPOCH="${SOURCE_DATE_EPOCH:-$(git -C "$root" show -s --format=%ct HEAD)}" \
    -v "$archive:/input/$(basename "$archive"):ro" -v "$out:/out" -v "$cache:/cache" -v "$here:/tools:ro" \
    "$image" bash -c "bash /tools/build.sh '/input/$(basename "$archive")' /out; status=\$?; chown -R $(id -u):$(id -g) /out /cache; exit \$status"
