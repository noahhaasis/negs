#!/bin/bash
# Render one case with the f64 CPU reference and other engines, then compare
# them pixel by pixel (CIEDE2000 on the encoded sRGB output).
#
#   pair.sh NAME PARAMS IMAGE FILM PAPER|scan [ENGINES...]
#
# ENGINES defaults to "ref gpu"; f32cpu is also available. Results go to
# $OUT/runs/NAME/<engine>.json and a one-line summary is printed.
# Build first: spektrafilm (f32) and spektrafilm-f64 in target/release.
set -e
HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$HERE/../.." && pwd)
OUT=${OUT:-$ROOT/target/gpu-cpu-quality}
PY=${PYTHON:-python3}
name=$1; params=$2; img=$3; film=$4; paper=$5; shift 5
engines=${*:-ref gpu}
R=$OUT/runs/$name; mkdir -p "$R"
if [ "$paper" = scan ]; then stock=(--scan-film); else stock=(--paper "$paper"); fi
for e in $engines; do
  case $e in
    ref) B=spektrafilm-f64; BE=cpu ;;
    gpu) B=spektrafilm; BE=wgpu ;;
    f32cpu) B=spektrafilm; BE=cpu ;;
  esac
  [ -f "$R/$e.f64" ] && continue
  (cd "$ROOT" && env SPEKTRAFILM_BACKEND=$BE "target/release/$B" process "$img" -o "$R/$e.tiff" \
    --film "$film" "${stock[@]}" --params "$params" --raw-out "$R/$e.f64" --timings > "$R/$e.log" 2>&1)
  rm -f "$R/$e.tiff"
done
wh=$(grep -o 'Saved: .*([0-9]*x[0-9]*)' "$R/ref.log" | sed -E 's/.*\(([0-9]+)x([0-9]+)\)/\1 \2/')
for e in $engines; do
  [ "$e" = ref ] && continue
  "$PY" -I "$HERE/compare.py" "$R/ref.f64" "$R/$e.f64" $wh "$R/$e.json" "$R/${e}_heat.png" > /dev/null
  echo "$name $e $(tr -d '\n ' < "$R/$e.json")"
done
[ -n "$KEEP_DUMPS" ] || rm -f "$R"/*.f64
