#!/bin/bash
# Model-checks the macOS canvas's frame pipeline (macos/Specs/FramePipeline.tla)
# with TLC. Fetches the TLA+ tools (pinned, checked) into target/tla once; needs Java.
#
#   scripts/check_frame_model.sh          one photograph, every interleaving (~4 min)
#   scripts/check_frame_model.sh full     two photographs (an hour or more)
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
version=1.7.4
sha256=936a262061c914694dfd669a543be24573c45d5aa0ff20a8b96b23d01e050e88
jar="$root/target/tla/tla2tools-$version.jar"

if [ ! -f "$jar" ]; then
    mkdir -p "$root/target/tla"
    curl -fsSL -o "$jar.part" "https://github.com/tlaplus/tlaplus/releases/download/v$version/tla2tools.jar"
    echo "$sha256  $jar.part" | shasum -a 256 -c - >/dev/null
    mv "$jar.part" "$jar"
fi

case "${1:-quick}" in
    quick) config=FramePipelineQuick.cfg ;;
    full) config=FramePipeline.cfg ;;
    *) echo "usage: $0 [quick|full]" >&2; exit 2 ;;
esac

cd "$root/macos/Specs"
exec java -XX:+UseParallelGC -cp "$jar" tlc2.TLC -workers auto -config "$config" \
    -metadir "$root/target/tla/states" FramePipeline
