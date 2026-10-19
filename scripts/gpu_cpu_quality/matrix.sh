#!/bin/bash
# Run pair.sh over every photo × stock for one params mode and append the
# result lines to $OUT/results-<mode>.txt (cases already there are skipped).
#
#   matrix.sh MODE [IMAGE...]
#
# MODE is a params file name without .json (det, colour, ...). Images default
# to the 18 October study set on this machine. Summarize with
#   summarize.py $OUT/results-<mode>.txt $OUT/runs summary.json
set -e
HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$HERE/../.." && pwd)
OUT=${OUT:-$ROOT/target/gpu-cpu-quality}
mode=$1; shift
if [ $# -eq 0 ]; then
  Z="$HOME/Downloads/Z 6"
  set -- "$Z/DSC_4990.NEF" "$Z/DSC_4997.NEF" "$Z/DSC_5003.NEF" "$Z/DSC_5009.NEF" "$Z/DSC_4985.NEF" \
    "$HOME/Downloads/7858115920.dng"
fi
stocks=(
  "kodak_portra_400 kodak_portra_endura"
  "kodak_gold_200 kodak_portra_endura"
  "fujifilm_pro_400h fujifilm_crystal_archive_typeii"
  "kodak_vision3_500t kodak_2383"
  "fujifilm_velvia_100 scan"
  "kodak_doublex kodak_2302"
  "kodak_trix scan"
)
mkdir -p "$OUT"
results=$OUT/results-$mode.txt
for img in "$@"; do
  tag=$(basename "$img" | sed 's/\..*//')
  for st in "${stocks[@]}"; do
    set -- $st
    name=m_${mode}_${tag}_$1
    grep -q "^$name " "$results" 2>/dev/null && continue
    "$HERE/pair.sh" "$name" "$HERE/params/$mode.json" "$img" "$1" "$2" >> "$results" || echo "$name FAILED" >> "$results"
  done
done
