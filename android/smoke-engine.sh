#!/usr/bin/env bash
# Runs the Android engine helper on a connected device or emulator over adb:
# the catalog, a render of a sample RAW and a save, through the same
# JSON-lines protocol the app speaks. Prints the helper's answers and pulls
# the saved JPEG into target/android-smoke/.
#
#   android/build-engine.sh && android/smoke-engine.sh [photo]
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
sdk="${ANDROID_HOME:-$HOME/Library/Android/sdk}"
adb="$sdk/platform-tools/adb"
photo="${1:-$root/web/sample/DSC_6377.dng}"
dir=/data/local/tmp/negs
out="$root/target/android-smoke"
mkdir -p "$out"

"$adb" shell mkdir -p "$dir/data"
"$adb" push "$root/android/app/src/main/jniLibs/arm64-v8a/libspektrafilm_native.so" "$dir/engine" >/dev/null
"$adb" shell chmod 755 "$dir/engine"
# The data once (profile-sources only build profiles).
if ! "$adb" shell test -d "$dir/data/profiles"; then
  for entry in "$root"/data/*; do
    [ "$(basename "$entry")" = profile-sources ] && continue
    "$adb" push "$entry" "$dir/data/" >/dev/null
  done
fi
name="photo.${photo##*.}"
"$adb" push "$photo" "$dir/$name" >/dev/null

cat > "$out/requests.jsonl" <<EOF
{"id":1,"command":"catalog"}
{"id":2,"command":"render","input":"$dir/$name","source_generation":1,"film":"kodak_portra_400","paper":"kodak_portra_endura","params":{},"limit_preview":true,"preview_path":"$dir/preview.rgba","interactive_scale":0.25}
{"id":3,"command":"render","input":"$dir/$name","source_generation":1,"film":"kodak_portra_400","paper":"kodak_portra_endura","params":{},"limit_preview":false,"preview_path":"$dir/preview.rgba"}
{"id":4,"command":"save","render_id":3,"output":"$dir/out.jpg","save_options":{"jpeg_quality":92}}
EOF
"$adb" push "$out/requests.jsonl" "$dir/requests.jsonl" >/dev/null

# RUST_LOG shows which backend the engine picked.
"$adb" shell "cd $dir && RUST_LOG=info XDG_CACHE_HOME=$dir/cache ./engine --data-dir $dir/data < requests.jsonl > responses.jsonl 2> stderr.txt; echo exit \$?"
"$adb" pull "$dir/responses.jsonl" "$out/" >/dev/null
"$adb" pull "$dir/stderr.txt" "$out/" >/dev/null
"$adb" pull "$dir/out.jpg" "$out/" >/dev/null 2>&1 || true
cut -c1-400 "$out/responses.jsonl"
echo "--- stderr"
tail -20 "$out/stderr.txt"
ls -l "$out"
