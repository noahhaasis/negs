#!/bin/sh
# Download the latest Negs release for Linux x86_64, verify its checksum and
# install it for the current user under ~/.local (no root needed).
#   curl -fsSL https://raw.githubusercontent.com/noahhaasis/negs/main/linux/get-negs.sh | sh
# NEGS_VERSION=0.11.0 installs a specific version instead of the latest.
set -eu

repo=noahhaasis/negs

for tool in curl tar sha256sum python3; do
    command -v "$tool" >/dev/null 2>&1 || { echo "negs: '$tool' is required" >&2; exit 1; }
done
[ "$(uname -m)" = x86_64 ] || { echo "negs: only x86_64 is supported" >&2; exit 1; }

if [ -n "${NEGS_VERSION:-}" ]; then
    version=$NEGS_VERSION
else
    # Releases before 1.0 are marked prereleases, so /releases/latest would not
    # find them; the newest entry of the release list is the one we want.
    version=$(curl -fsSL "https://api.github.com/repos/$repo/releases?per_page=1" |
        sed -n 's/.*"tag_name": *"v\([0-9][^"]*\)".*/\1/p' | head -n 1)
    [ -n "$version" ] || { echo "negs: could not find the latest release" >&2; exit 1; }
fi

base="https://github.com/$repo/releases/download/v$version"
bundle="negs-$version-linux-x86_64.tar.gz"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

echo "Downloading Negs $version…"
curl -fL --progress-bar -o "$work/$bundle" "$base/$bundle"
curl -fsSL -o "$work/SHA256SUMS-linux" "$base/SHA256SUMS-linux"
(cd "$work" && grep " $bundle\$" SHA256SUMS-linux | sha256sum -c -) ||
    { echo "negs: checksum mismatch, nothing was installed" >&2; exit 1; }

mkdir "$work/bundle"
tar -xzf "$work/$bundle" -C "$work/bundle"
python3 "$work/bundle/share/negs/install.py" --from "$work/bundle"

echo
echo "Negs $version is installed. Start it from your app launcher, or run: ~/.local/bin/negs"
echo "Negs needs GTK 4.16+, libadwaita 1.6+, libgfortran/libgomp and a Vulkan driver. OpenBLAS is bundled."
echo "To remove it: python3 ~/.local/share/negs/install.py --uninstall"
