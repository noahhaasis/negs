#!/usr/bin/env bash
# Makes the Newsreader subsets the macOS app bundles (macos/Fonts/).
#
# Source: assets/fonts/newsreader/, the variable fonts from google/fonts
# (ofl/newsreader), by Production Type, under the SIL Open Font License 1.1
# with no Reserved Font Name, so the subset keeps the name. The licence ships
# beside the fonts.
#
# Kept: Latin-1 and Latin Extended-A (file names are often Polish, Czech or
# Turkish), the punctuation, arrows and minus the interface uses, every
# OpenType feature (the design uses tnum and lnum), the opsz axis whole, and
# wght cut to 400–700. Anything else falls back through Core Text's cascade.
#
# Needs hb-subset (HarfBuzz; `brew install harfbuzz`).
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
source_dir="$repo_root/assets/fonts/newsreader"
out_dir="$repo_root/macos/Fonts"
mkdir -p "$out_dir"

unicodes="U+0020-007E,U+00A0-017F,U+02C6,U+02DC,U+2009,U+200A,U+202F,U+2013-2014,U+2018-201E,U+2022,U+2026,U+2032-2033,U+2039-203A,U+2044,U+20AC,U+2190-2193,U+2212,U+21BA"

subset() {
    hb-subset "$source_dir/$1" --layout-features='*' --instance="wght=400:700" \
        --unicodes="$unicodes" -o "$out_dir/$2"
}
subset Newsreader.ttf Newsreader-Negs.ttf
subset Newsreader-Italic.ttf Newsreader-Negs-Italic.ttf
cp "$source_dir/OFL-Newsreader.txt" "$out_dir/OFL-Newsreader.txt"
ls -l "$out_dir"
