#!/usr/bin/env bash
set -euo pipefail
root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
source "$root/linux/core-env.sh"
if ! command -v cargo >/dev/null; then
  export RUSTUP_HOME="$root/target/linux-toolchain/rustup"
  export CARGO_HOME="$root/target/linux-toolchain/cargo"
  export PATH="$CARGO_HOME/bin:$PATH"
fi
export CARGO_TARGET_DIR="$root/target/linux-ui"
command="$1"
shift
exec cargo +1.93.1 "$command" --manifest-path "$root/linux/Cargo.toml" "$@"
