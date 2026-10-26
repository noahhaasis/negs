#!/usr/bin/env bash
set -euo pipefail
root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
source "$root/linux/core-env.sh"
exec "$root/target/linux-ui/release/negs" "$@"
