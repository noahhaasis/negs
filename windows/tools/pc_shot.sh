#!/bin/bash
# Screenshots of the Windows app on the PC, copied back to the Mac.
# Usage: windows/tools/pc_shot.sh <job.json> <local out dir>
# The job's "out" is the PC folder; see pc/shot.ps1 for the job format.
# Install the helpers once per change: windows/tools/pc_install.sh
set -euo pipefail
job="$1"; dest="$2"
name="$(basename "$job")"
scp -q "$job" "pc:C:/Users/Noah/.negs/shots/$name"
"$(dirname "$0")/pc_console.sh" "& C:\\Users\\Noah\\.negs\\shot.ps1 -JobFile C:\\Users\\Noah\\.negs\\shots\\$name" 300
out="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["out"])' "$job")"
mkdir -p "$dest"
scp -q "pc:$(echo "$out" | tr '\\' '/')/*.png" "$dest/" && ls "$dest"
