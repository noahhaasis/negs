#!/usr/bin/env bash
# Install a Flatpak bundle for this user and check it inside the sandbox:
# payload files, CPU engine renders and exports, and a GUI start and quit.
#   linux/flatpak/verify.sh BUNDLE
# Without a display, run under xvfb-run (the X11 fallback socket is used).
set -euo pipefail
root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"
bundle=$(readlink -f "$1")
app=io.github.noahhaasis.negs
flatpak install --user -y --noninteractive --reinstall "$bundle"
flatpak info --user --show-permissions "$app"
# Read-only access to the verification scripts only, never home or host.
flatpak run --user --filesystem="$root/linux:ro" --filesystem="$root/scripts:ro" \
    --env=OPENBLAS_NUM_THREADS=1 --command=python3 "$app" \
    "$root/linux/verify_package.py" --prefix /app --format flatpak
log=$(mktemp)
status=0
timeout 120 flatpak run --user --env=NEGS_PROFILE_DIR=/tmp/negs-smoke --env=NEGS_QUIT_AFTER_MS=12000 \
    --env=SPEKTRAFILM_BACKEND=cpu "$app" --fresh > "$log" 2>&1 || status=$?
cat "$log"
if [ "$status" -ne 0 ] || grep -E 'CRITICAL|panicked|cannot open shared object' "$log"; then
    echo "GUI smoke test failed (exit $status)" >&2
    exit 1
fi
echo 'Flatpak sandbox checks passed.'
