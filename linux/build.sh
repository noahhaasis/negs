#!/usr/bin/env bash
set -euo pipefail
root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"
source linux/core-env.sh
export CARGO_TARGET_DIR="${NEGS_ENGINE_TARGET_DIR:-$root/target}"
extra=()
if [ "${NEGS_CARGO_OFFLINE:-0}" = 1 ]; then extra=(--offline); fi
cargo +1.93.1 build --release --locked "${extra[@]}" -p spektrafilm-cli --bin spektrafilm
cargo +1.93.1 build --release --locked "${extra[@]}" -p spektrafilm-native
# Keep precision-f64 out of the Fast/helper build.
cargo +1.93.1 build --release --locked "${extra[@]}" -p spektrafilm-cli --features precision-f64 --bin spektrafilm-f64
bash linux/cargo.sh build --release --locked "${extra[@]}"
