#!/bin/bash
# Statistical grain comparison for one case (see grain.py for the method).
#
#   grain_case.sh RAW FILM PAPER|scan [PARAMS_SUFFIX]
#
# Uses params/grain_on$SUFFIX.json and params/grain_off$SUFFIX.json. Renders
# six images: CPU realization A (full frame), CPU realization B (input shifted
# by one pixel), and the GPU, each with grain on and off.
set -e
HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$HERE/../.." && pwd)
OUT=${OUT:-$ROOT/target/gpu-cpu-quality}
PY=${PYTHON:-python3}
raw=$1; film=$2; paper=$3; suffix=${4:-}
tag=$(basename "$raw" | sed 's/\..*//')
mkdir -p "$OUT/inputs"
A=$OUT/inputs/${tag}_A.tif; B=$OUT/inputs/${tag}_B.tif
[ -f "$A" ] || "$ROOT/target/release/decode_raw_gui" "$raw" "$A" > /dev/null
[ -f "$B" ] || "$PY" -I -c "import tifffile, sys; a = tifffile.imread(sys.argv[1]); tifffile.imwrite(sys.argv[2], a[1:, 1:])" "$A" "$B"
R=$OUT/runs/grain_${tag}_$film$suffix; mkdir -p "$R"
if [ "$paper" = scan ]; then stock=(--scan-film); else stock=(--paper "$paper"); fi
run() { # name backend binary input params
  [ -f "$R/$1.f64" ] && return
  (cd "$ROOT" && env SPEKTRAFILM_BACKEND=$2 "target/release/$3" process "$4" -o "$R/$1.tiff" --film "$film" \
    "${stock[@]}" --params "$HERE/params/$5.json" --raw-out "$R/$1.f64" --timings > "$R/$1.log" 2>&1)
  rm -f "$R/$1.tiff"
}
run cpuA_ng cpu spektrafilm-f64 "$A" grain_off$suffix
run cpuA_g cpu spektrafilm-f64 "$A" grain_on$suffix
run cpuB_ng cpu spektrafilm-f64 "$B" grain_off$suffix
run cpuB_g cpu spektrafilm-f64 "$B" grain_on$suffix
run gpu_ng wgpu spektrafilm "$A" grain_off$suffix
run gpu_g wgpu spektrafilm "$A" grain_on$suffix
wh=$(grep -o 'Saved: .*([0-9]*x[0-9]*)' "$R/cpuA_ng.log" | sed -E 's/.*\(([0-9]+)x([0-9]+)\)/\1 \2/')
"$PY" -I "$HERE/grain.py" "$R" $wh "$R/grain.json"
"$PY" -I "$HERE/crops.py" "$R" $wh
[ -n "$KEEP_DUMPS" ] || rm -f "$R"/*.f64
"$PY" -I "$HERE/grain_summary.py" "$R/grain.json"
