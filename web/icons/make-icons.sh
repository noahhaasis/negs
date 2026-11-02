#!/usr/bin/env bash
# The web app's icons, from the desktop icon (assets/negs-icon.svg and .png):
# its rounded icon for browsers, and full-bleed squares of its background for
# iOS (which rounds the corners itself) and Android's maskable icon (the
# film inside the circle every launcher's mask keeps). Needs rsvg-convert
# and ImageMagick.
set -euo pipefail
cd "$(dirname "$0")"
assets=../../assets
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
# The squircle's background and glow over the whole canvas, without its
# rounding, shadow or rim.
sed -e 's|<rect x="100" y="100" width="824" height="824" rx="185" fill="url(#bg)" filter="url(#shell)"/>|<rect x="-400" y="-400" width="1824" height="1824" fill="url(#bg)"/>|' \
    -e 's|<clipPath id="squircle"><rect x="100" y="100" width="824" height="824" rx="185"/></clipPath>|<clipPath id="squircle"><rect x="-400" y="-400" width="1824" height="1824"/></clipPath>|' \
    -e 's|<rect x="100" y="100" width="824" height="824" fill="url(#glow)"/>|<rect x="-400" y="-400" width="1824" height="1824" fill="url(#glow)"/>|' \
    -e '/<rect x="101.5"/d' "$assets/negs-icon.svg" > "$tmp/flat.svg"
grep -q 'rx="185"' "$tmp/flat.svg" && { echo "negs-icon.svg changed; update make-icons.sh" >&2; exit 1; }
sed 's/viewBox="0 0 1024 1024"/viewBox="100 100 824 824"/' "$tmp/flat.svg" > "$tmp/full.svg"
sed 's/viewBox="0 0 1024 1024"/viewBox="-60 -70 1144 1144"/' "$tmp/flat.svg" > "$tmp/maskable.svg"
rsvg-convert -w 180 -h 180 "$tmp/full.svg" | magick - -strip -depth 8 PNG24:apple-touch-icon.png
rsvg-convert -w 512 -h 512 "$tmp/maskable.svg" | magick - -strip -depth 8 PNG24:icon-maskable-512.png
for size in 192 512; do
  magick "$assets/negs-icon.png" -filter Lanczos -resize ${size}x$size -strip -depth 8 PNG32:icon-$size.png
done
