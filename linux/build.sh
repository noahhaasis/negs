#!/usr/bin/env bash
# Builds the three engine executables and the GTK app. Distribution builds may
# set NEGS_TOOLCHAIN= (empty: plain cargo from an SDK), NEGS_ENGINE_TARGET_DIR,
# NEGS_UI_TARGET_DIR and NEGS_PACKAGED=1 (no source-checkout fallbacks).
set -euo pipefail
root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"
source linux/core-env.sh
export CARGO_TARGET_DIR="${NEGS_ENGINE_TARGET_DIR:-$root/target}"
read -r -a toolchain <<< "${NEGS_TOOLCHAIN-+1.93.1}"
extra=()
if [ "${NEGS_CARGO_OFFLINE:-0}" = 1 ]; then extra=(--offline); fi
ui=()
if [ "${NEGS_PACKAGED:-0}" = 1 ]; then ui=(--features packaged); fi
cargo "${toolchain[@]}" -vV
cargo "${toolchain[@]}" build --release --locked "${extra[@]}" -p spektrafilm-cli --bin spektrafilm
cargo "${toolchain[@]}" build --release --locked "${extra[@]}" -p spektrafilm-native
# Keep precision-f64 out of the Fast/helper build.
cargo "${toolchain[@]}" build --release --locked "${extra[@]}" -p spektrafilm-cli --features precision-f64 --bin spektrafilm-f64
bash linux/cargo.sh build --release --locked "${extra[@]}" "${ui[@]}"
