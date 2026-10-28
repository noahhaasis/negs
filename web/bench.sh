#!/usr/bin/env bash
# Render through the app's automation hook in a fresh windowed Chrome with
# real WebGPU, and print the timings: web/bench.sh <name> '<query>'
#   web/bench.sh card-1200 'image=results/card.jpg&backend=gpu&max=1200&runs=8'
# Needs web/serve.mjs running. Writes web/results/<name>.{json,ppm} (and
# <name>-canvas.ppm when the frame was presented). A Chrome profile goes in
# $BENCH_PROFILE (default: a temp dir); $BENCH_PAGE picks another page than index.html.
set -euo pipefail
web=$(cd "$(dirname "$0")" && pwd)
name=$1 query=$2
profile=${BENCH_PROFILE:-$(mktemp -d)}
rm -f "$web/results/$name.json"
# x11: on Wayland, Vulkan breaks 2D canvas uploads. The binary directly: the
# google-chrome-stable wrapper adds the user's flags.
/opt/google/chrome/chrome --ozone-platform=x11 --window-size=900,700 --user-data-dir="$profile" \
  --no-first-run --no-default-browser-check --enable-unsafe-webgpu --enable-features=Vulkan \
  --ignore-gpu-blocklist "http://localhost:8137/${BENCH_PAGE-}?$query&name=$name" >/dev/null 2>&1 &
for _ in $(seq 240); do [[ -f $web/results/$name.json ]] && break; sleep 0.5; done
sleep 0.3
pgrep -f "^/opt/google/chrome/chrome .*$profile" | xargs -r kill
node -e '
const d = JSON.parse(require("fs").readFileSync(process.argv[1]));
if (d.error) { console.log("ERROR", d.error); process.exit(1); }
const f = (xs) => (xs ?? []).slice(1).map((x) => x.toFixed(0)).join(" ");
console.log(`${process.argv[2]} ${d.backend} ${d.width}x${d.height} presenting=${d.presenting} first ${d.times[0].toFixed(0)} warm ${f(d.times)} frames ${f(d.frames)}`);
console.log(`  last: ${d.stages.at(-1)}`);
' "$web/results/$name.json" "$name"
