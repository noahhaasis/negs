#!/bin/bash
# Effects that are off by default, f64 CPU reference vs GPU, deterministic.
# Each params/fx/<name>.json holds overrides merged over params/det.json
# (grain and glare off). Appends result lines to $OUT/results-fx.txt.
#
#   effects.sh [EFFECT...]        default: every params/fx/*.json
#
# Photos: DSC_5009 and DSC_4990, Portra 400 -> Portra Endura (the print
# path, which enlarger diffusion, preflash and the print-curve morph need).
# Each line is pair.sh output ("fx_<effect>_<photo> gpu {stats}").
set -e
HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$HERE/../.." && pwd)
OUT=${OUT:-$ROOT/target/gpu-cpu-quality}
PY=${PYTHON:-python3}
mkdir -p "$OUT/params-fx"
results=$OUT/results-fx.txt
if [ $# -eq 0 ]; then set -- $(cd "$HERE/params/fx" && ls *.json | sed 's/\.json$//'); fi
Z="$HOME/Downloads/Z 6"
for fx in "$@"; do
  merged=$OUT/params-fx/$fx.json
  "$PY" -I -c '
import json, sys
def merge(a, b):
    for k, v in b.items():
        if isinstance(v, dict) and isinstance(a.get(k), dict): merge(a[k], v)
        else: a[k] = v
    return a
base = json.load(open(sys.argv[1]))
json.dump(merge(base, json.load(open(sys.argv[2]))), open(sys.argv[3], "w"), indent=1)
' "$HERE/params/det.json" "$HERE/params/fx/$fx.json" "$merged"
  for img in "$Z/DSC_5009.NEF" "$Z/DSC_4990.NEF"; do
    name=fx_${fx}_$(basename "$img" | sed 's/\..*//')
    grep -q "^$name " "$results" 2>/dev/null && continue
    "$HERE/pair.sh" "$name" "$merged" "$img" kodak_portra_400 kodak_portra_endura >> "$results" || echo "$name FAILED" >> "$results"
  done
done
