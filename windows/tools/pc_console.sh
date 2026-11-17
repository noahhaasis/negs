#!/bin/bash
# Run a PowerShell command line in Noah's console session on the PC (GUI visible),
# wait for it, and print its output. ssh is session 0 and has no desktop.
# Usage: windows/tools/pc_console.sh '<powershell command>' [timeout seconds]
# Example: windows/tools/pc_console.sh '& C:\Users\Noah\.negs\startup.ps1 -A C:\...\Negs.exe -Runs 6'
set -euo pipefail
cmd="$1"; limit="${2:-600}"
tmp="$(mktemp -d)"
printf '%s\r\n' "\$ErrorActionPreference='Continue'" "try { $cmd *>&1 | Out-File -Encoding utf8 C:\\Users\\Noah\\.negs\\task.log } catch { \$_ | Out-File -Append -Encoding utf8 C:\\Users\\Noah\\.negs\\task.log }" "'done' | Out-File C:\\Users\\Noah\\.negs\\task.done" > "$tmp/task.ps1"
scp -q "$tmp/task.ps1" pc:C:/Users/Noah/.negs/task.ps1
rm -rf "$tmp"
ssh pc 'del /q C:\Users\Noah\.negs\task.done C:\Users\Noah\.negs\task.log 2>nul & schtasks /create /it /ru Noah /sc once /st 00:00 /tn negs-shot /tr "powershell -NoProfile -WindowStyle Hidden -ExecutionPolicy Bypass -File C:\Users\Noah\.negs\task.ps1" /f >nul && schtasks /run /tn negs-shot >nul' 2>/dev/null
start=$(date +%s)
until ssh pc 'if exist C:\Users\Noah\.negs\task.done (echo yes) else (echo no)' 2>/dev/null | grep -q yes; do
  if (( $(date +%s) - start > limit )); then echo "pc_console: timed out after ${limit}s" >&2; break; fi
  sleep 3
done
ssh pc 'type C:\Users\Noah\.negs\task.log' 2>/dev/null | iconv -f utf-8 -t utf-8 -c | tr -d '\r' | sed '1s/^\xEF\xBB\xBF//'
