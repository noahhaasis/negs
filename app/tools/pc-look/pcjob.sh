#!/bin/bash
# pcjob.sh <job.ps1> <timeout s>: runs it in the PC's console session, waits for job.done, prints job.out.
set -u
D=C:/Users/Noah/.negs/scratch/gpui-look
scp -q "$1" pc:$D/job.ps1 2>/dev/null || { echo "scp failed"; exit 1; }
ssh pc "del /q C:\\Users\\Noah\\.negs\\scratch\\gpui-look\\job.done 2>nul & schtasks /run /tn negs-gpui-look" >/dev/null 2>&1
END=$(( $(date +%s) + $2 ))
sleep 3
while [ "$(date +%s)" -lt $END ]; do
  if ssh pc "if exist C:\\Users\\Noah\\.negs\\scratch\\gpui-look\\job.done echo yes" 2>/dev/null | grep -q yes; then
    ssh pc "type C:\\Users\\Noah\\.negs\\scratch\\gpui-look\\job.out" 2>/dev/null; echo "[job done]"; exit 0
  fi
  sleep 4
done
echo "[job timed out after $2 s]"; exit 2
