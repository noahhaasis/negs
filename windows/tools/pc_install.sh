#!/bin/bash
# Copy the PC helpers (shot.ps1, startup.ps1) to C:\Users\Noah\.negs.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
ssh pc 'if not exist C:\Users\Noah\.negs\shots mkdir C:\Users\Noah\.negs\shots' 2>/dev/null
scp -q "$here/pc/shot.ps1" "$here/pc/startup.ps1" pc:C:/Users/Noah/.negs/
echo installed
