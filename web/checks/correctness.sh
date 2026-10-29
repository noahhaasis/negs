#!/usr/bin/env bash
# The browser's WebGPU renders against the f64 CPU reference (the engine
# the desktop apps export with) and native wgpu, case by case:
#   web/checks/correctness.sh [case,case,...]
# Needs web/serve.mjs running, target/release/spektrafilm and
# spektrafilm-f64 (see docs/web-wasm-handoff.md) and uv (for numpy and
# colour-science). Renders go to $OUT (default target/web-correctness);
# SKIP_BROWSER=1 reuses the browser's renders there, and the CLI's renders
# are kept until their edit changes. Prints one line per case and writes
# $OUT/summary.json.
# The photographs are gitignored test inputs in web/results: card.jpg and
# sony.arw (see docs/web-wasm-handoff.md), and hills, coast and cliffs.jpg,
# three Unsplash photographs (andrew-ridley-Kt5hRENuotI,
# nils-nedel-FauBUuNPq5k, sean-kuriyan-sKm0RZjOgIE) fitted in 1600 px with
# `magick <in> -resize 1600x1600 -quality 95 <out>`, which keeps the profile.
set -euo pipefail
web=$(cd "$(dirname "$0")/.." && pwd)
root=$(cd "$web/.." && pwd)
OUT=${OUT:-$root/target/web-correctness}
mkdir -p "$OUT"
only=${1-}
cases=${CASES:-checks/correctness-cases.json}
if [[ -z ${SKIP_BROWSER-} ]]; then
  rm -f "$web"/results/c-*
  CDP_TMP=$OUT node "$web/cdp.mjs" "http://localhost:8137/?cases=$cases${only:+&only=$only}" "$web/checks/correctness.js" 3000 > "$OUT/browser.log"
  grep -q '\[script failed\]\|\[exception\]' "$OUT/browser.log" && { cat "$OUT/browser.log"; exit 1; }
  mv "$web"/results/c-* "$OUT/"
fi
source "$root/linux/core-env.sh"
for edit in "$OUT"/c-*-edit.json; do
  name=$(basename "$edit" -edit.json); name=${name#c-}
  [[ -n $only && ",$only," != *",$name,"* ]] && continue
  read -r image film paper scan < <(node -e '
    const e = JSON.parse(require("fs").readFileSync(process.argv[1]));
    console.log(e.image, e.film, e.paper, e.params.io.scan_film ? 1 : 0);' "$edit")
  node -e 'const e = JSON.parse(require("fs").readFileSync(process.argv[1])); process.stdout.write(JSON.stringify(e.params))' "$edit" > "$OUT/$name.params.json"
  if [[ $scan == 1 ]]; then stock=(--scan-film); else stock=(--paper "$paper"); fi
  for engine in ref gpu; do
    case $engine in
      ref) bin=$root/target/release/spektrafilm-f64 backend=cpu ;;
      gpu) bin=$root/target/release/spektrafilm backend=wgpu ;;
    esac
    # Rendered already for this edit?
    if [[ -f $OUT/$name.$engine.f64 ]] && cmp -s "$OUT/$name.params.json" "$OUT/$name.$engine.params"; then continue; fi
    (cd "$root" && SPEKTRAFILM_ROOT=$root SPEKTRAFILM_BACKEND=$backend "$bin" process "web/$image" -o "$OUT/$name.$engine.tiff" \
      --film "$film" "${stock[@]}" --params "$OUT/$name.params.json" --no-metadata --raw-out "$OUT/$name.$engine.f64" > "$OUT/$name.$engine.log" 2>&1) \
      || { echo "$name: $engine failed"; tail -3 "$OUT/$name.$engine.log"; continue 2; }
    rm -f "$OUT/$name.$engine.tiff"
    cp "$OUT/$name.params.json" "$OUT/$name.$engine.params"
  done
done
uv run -q --no-project --with numpy --with colour-science python -I -W ignore "$web/checks/correctness.py" "$OUT" ${only:+"$only"}
