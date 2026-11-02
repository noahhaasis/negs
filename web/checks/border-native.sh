#!/usr/bin/env bash
# The browser's bordered exports against native code, for the cases
# web/checks/border.mjs saved (web/results/border-cases.json and its PPMs):
#  - native: spektrafilm-io's add_border (and extend_for_instagram) on the
#    browser's own unbordered export (examples/border_bytes.rs), which must
#    equal the browser's bordered bytes exactly;
#  - cli: the CLI's export of the same edit with the same --border flags,
#    whose size must be the same and its pixels within the agreement of
#    docs/web-wasm.md item 13.
#   web/checks/border-native.sh
# Needs target/release/spektrafilm (see docs/web-wasm-handoff.md). Writes to
# $OUT (default target/web-border).
set -euo pipefail
web=$(cd "$(dirname "$0")/.." && pwd)
root=$(cd "$web/.." && pwd)
OUT=${OUT:-$root/target/web-border}
mkdir -p "$OUT"
source "$root/linux/core-env.sh"
(cd "$root" && cargo build -q --release -p spektrafilm-io --example border_bytes)
node -e '
  const cases = JSON.parse(require("fs").readFileSync(process.argv[1]));
  for (const c of cases) {
    const o = c.options;
    const flags = ["--border", c.style, "--keyline-width", o.line, "--border-color", o.color, "--keyline-color", o.line_color,
      ...(o.width !== undefined ? ["--border-width", o.width] : []), ...(c.instagram ? ["--instagram"] : [])];
    console.log([c.photo, c.image, c.style + (c.instagram ? "-ig" : ""), JSON.stringify(o), c.film, c.paper, c.instagram ? "--instagram" : "-",
      JSON.stringify(c.params), flags.join(" ")].join("\t"));
  }' "$web/results/border-cases.json" |
while IFS=$'\t' read -r photo image key options film paper instagram params flags; do
  ig=(); [[ $instagram == --instagram ]] && ig=(--instagram)
  "$root/target/release/examples/border_bytes" "$web/results/border-$photo.ppm" "$OUT/native-$photo-$key.ppm" \
    "${key%-ig}" "$options" "$film" "${ig[@]}" "$root/data" 2> /dev/null
  native=$(node "$web/compare.mjs" "$web/results/border-$photo-$key.ppm" "$OUT/native-$photo-$key.ppm")
  printf '%s' "$params" > "$OUT/$photo.params.json"
  # shellcheck disable=SC2086 # the flags are words
  (cd "$root" && SPEKTRAFILM_ROOT=$root SPEKTRAFILM_BACKEND=wgpu target/release/spektrafilm process "web/$image" -o "$OUT/cli-$photo-$key.png" \
    --film "$film" --paper "$paper" --params "$OUT/$photo.params.json" --no-metadata $flags > "$OUT/cli-$photo-$key.log" 2>&1)
  magick "$OUT/cli-$photo-$key.png" -depth 8 "$OUT/cli-$photo-$key.ppm"
  cli=$(node "$web/compare.mjs" "$web/results/border-$photo-$key.ppm" "$OUT/cli-$photo-$key.ppm")
  echo "$photo $key  native: ${native#*: }  cli: ${cli#*: }"
done
