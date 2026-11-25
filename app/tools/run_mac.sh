#!/bin/bash
# Runs the GPUI app on a scratch photo folder with a scratch profile, and
# answers the app's NEGS_BENCH_SHOTS handshake with real window captures.
#
#   app/tools/run_mac.sh <Negs binary> <photo folder> <scratch dir> <bench.json|none> <shots dir|none> <timeout s>
#
# The profile is <scratch dir>/profile. Never point it at real photos: the
# bench rates, flags and filters them. Builds app/tools/winid on first use.
set -u
APP=$1; PHOTOS=$2; SCRATCH=$3; BENCH=$4; SHOTS=$5; LIMIT=$6
HERE=$(cd "$(dirname "$0")" && pwd)
WINID=$SCRATCH/winid
[ -x "$WINID" ] || swiftc -O -o "$WINID" "$HERE/winid.swift" || exit 1
mkdir -p "$SCRATCH/profile"
export NEGS_PROFILE_DIR=$SCRATCH/profile NEGS_ALLOW_MULTIPLE=1 SPEKTRAFILM_ROOT=${SPEKTRAFILM_ROOT:-/Users/noahhaasis/Projects/negs}
[ "$BENCH" != none ] && export NEGS_FRAME_BENCH=$BENCH
[ "$SHOTS" != none ] && export NEGS_BENCH_SHOTS=$SHOTS && rm -rf "$SHOTS" && mkdir -p "$SHOTS"
"$APP" "$PHOTOS" > "$SCRATCH/app.log" 2>&1 &
PID=$!
echo "pid $PID"
END=$(( $(date +%s) + LIMIT ))
while kill -0 $PID 2>/dev/null; do
  if [ "$SHOTS" != none ]; then
    for r in "$SHOTS"/*.ready; do
      [ -e "$r" ] || continue
      n=$(basename "$r" .ready)
      [ -e "$SHOTS/$n.done" ] && continue
      # By PID only: Noah's own Negs.app may be running.
      osascript -e "tell application \"System Events\" to set frontmost of (first process whose unix id is $PID) to true" >/dev/null 2>&1
      sleep 0.3
      screencapture -x -o -l "$("$WINID" $PID)" "$SHOTS/$n.png"
      touch "$SHOTS/$n.done"
      echo "shot $n"
    done
  fi
  if [ "$(date +%s)" -ge $END ]; then echo "timeout, killing"; kill $PID; break; fi
  sleep 0.1
done
wait $PID 2>/dev/null
echo "exit $?"
