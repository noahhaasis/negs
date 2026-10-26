#!/usr/bin/env bash
set -euo pipefail
root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
source "$root/linux/core-env.sh"
export SPEKTRAFILM_ROOT="$root"
export NEGS_DATA_DIR="${NEGS_DATA_DIR:-$root/data}"
exec "$root/target/linux-ui/release/negs" "$@"
