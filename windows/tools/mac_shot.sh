#!/bin/bash
# macOS reference screenshots for Windows parity, from an isolated copy of the
# dev app (its own bundle id, HOME and defaults), so Noah's app and settings
# are never touched. Targets the process by PID.
# Usage: windows/tools/mac_shot.sh <steps file> <out dir> [photo] [theme]
#   theme: darkroom|contactSheet|lightTable|daylight|plain|plainDark (default darkroom)
# Steps file, one per line:
#   shot <name>            capture the window to <out>/<name>.png
#   shotfront <name>       capture the app's frontmost window instead (Settings)
#   key <char> [cmd,opt,shift,ctrl]   keystroke, e.g. "key g" or "key , cmd"
#   code <keycode> [mods]  key code, e.g. "code 53" (Esc)
#   click <x> <y> [n]      window-relative points, n clicks
#   clickfront <x> <y> [n] the same in the frontmost window
#   wait <seconds>
# Needs: dist/negs-macos-dev/Negs.app (scripts/package_macos_app.sh --dev).
set -euo pipefail
steps="$1"; out="$2"; photo="${3:-}"; theme="${4:-darkroom}"
repo="$(cd "$(dirname "$0")/../.." && pwd)"
base="$HOME/.negs/windows-parity/mac"
bin="$base/winshot"; app="$base/Negs.app"; id="dev.negs.parityref"
mkdir -p "$base" "$out"
if [ ! -x "$bin" ] || [ "$repo/windows/tools/mac/winshot.swift" -nt "$bin" ]; then
  swiftc -O -o "$bin" "$repo/windows/tools/mac/winshot.swift"
fi
src="$repo/dist/negs-macos-dev/Negs.app"
if [ ! -d "$app" ] || [ "$src/Contents/MacOS/NegsApp" -nt "$app/Contents/MacOS/NegsApp" ]; then
  rm -rf "$app"; cp -R "$src" "$app"
  /usr/libexec/PlistBuddy -c "Set :CFBundleIdentifier $id" "$app/Contents/Info.plist"
  codesign --force --deep -s - "$app" >/dev/null 2>&1
fi
home="$base/home"; rm -rf "$home"; mkdir -p "$home"
defaults delete "$id" >/dev/null 2>&1 || true
defaults write "$id" appTheme "$theme"
defaults write "$id" NSWindow\ Frame\ main "40 40 1440 900 0 0 3000 2000" >/dev/null 2>&1 || true
HOME="$home" CFFIXED_USER_HOME="$home" "$app/Contents/MacOS/NegsApp" ${photo:+"$photo"} >"$base/app.log" 2>&1 &
pid=$!
trap 'kill $pid 2>/dev/null || true' EXIT
for _ in $(seq 1 60); do "$bin" window "$pid" >/dev/null 2>&1 && break; sleep 0.5; done
osascript -e "tell application \"System Events\" to tell (first process whose unix id is $pid)
  set frontmost to true
  try
    set position of window 1 to {40, 40}
    set size of window 1 to {1440, 900}
  end try
end tell" >/dev/null
sleep 5
front() { osascript -e "tell application \"System Events\" to set frontmost of (first process whose unix id is $pid) to true" >/dev/null; }
mods() {
  [ -z "${1:-}" ] && return
  local m=() IFS=,
  for x in $1; do
    case "$x" in cmd) x=command ;; opt) x=option ;; ctrl) x=control ;; esac
    m+=("$x down")
  done
  local IFS=', '; echo " using {${m[*]}}"
}
while read -r verb a b c; do
  case "$verb" in
    ''|'#'*) ;;
    shot|shotfront) front; sleep 0.4
      if [ "$verb" = shot ]; then set -- $("$bin" window "$pid"); else set -- $("$bin" front "$pid"); fi
      screencapture -x -o -l "$1" "$out/$a.png"; echo "shot $out/$a.png" ;;
    key) front; osascript -e "tell application \"System Events\" to keystroke \"$a\"$(mods "${b:-}")" ;;
    code) front; osascript -e "tell application \"System Events\" to key code $a$(mods "${b:-}")" ;;
    click|clickfront) front
      if [ "$verb" = click ]; then set -- $("$bin" window "$pid"); else set -- $("$bin" front "$pid"); fi
      # System Events clicks (accessibility); CGEvents from here don't reach SwiftUI.
      for _ in $(seq 1 "${c:-1}"); do
        osascript -e "tell application \"System Events\" to tell (first process whose unix id is $pid) to click at {$(( $2 + a )), $(( $3 + b ))}" >/dev/null
      done ;;
    wait) sleep "$a" ;;
  esac
  sleep 0.6
done < "$steps"
