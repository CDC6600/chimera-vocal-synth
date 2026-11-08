#!/bin/bash
# CHIMERA in a real host: REAPER, on this Mac, loads the bundles that `build.sh --install` put in
# place, renders what each mode does (../host-test/host_test.lua, judged by ../host-test/check.ts),
# and opens each editor for this script to look at and click. GitHub's Macs have a display and let
# scripts capture it and post clicks, which is what makes this work on CI. See docs/plugin.md,
# "Testing in a host". Screenshots also go to the log: the workflow uploads artifacts before this runs.
set -euo pipefail
root=$(git rev-parse --show-toplevel)
here="$root/crates/chimera-plugin"
version=785
sha=4501a1280ef5ca33e1ad32974917281bfb31514bcfc043fe3e586cba6cba4686
out="$root/target/host-test"
work=$(mktemp -d)
rm -rf "$out"
mkdir -p "$out"

shot() { # name
  screencapture -x "$out/$1.png" || { echo "no screenshot: $1"; return 0; }
  sips -Z 1400 -s format jpeg -s formatOptions 60 "$out/$1.png" --out "$out/$1.jpg" >/dev/null
  echo "=== screenshot $1 begin"
  base64 -b 200 -i "$out/$1.jpg"
  echo "=== screenshot $1 end"
}

# the editor's window: x y width height
bounds() {
  osascript -e 'tell application "System Events" to tell process "REAPER"
    repeat with w in windows
      if name of w contains "CHIMERA" then
        set {x, y} to position of w
        set {cx, cy} to size of w
        return (x as text) & " " & (y as text) & " " & (cx as text) & " " & (cy as text)
      end if
    end repeat
  end tell' 2>&1 || true
}

# Where these are in the page at 1280x800: tools/plugin-e2e.ts's harness, measured.
BEATBOX_X=624
BEATBOX_Y=26
VIEW_W=1280
VIEW_H=800

editor() { # format
  local fmt=$1 x y w h left top gx gy
  osascript -e 'tell application "System Events" to tell process "REAPER"' \
    -e 'set frontmost to true' \
    -e 'get name of every window' \
    -e 'end tell' 2>&1 || true
  shot "$fmt-open"
  read -r x y w h <<<"$(bounds)" || true
  echo "$fmt window: ${x:-?} ${y:-?} ${w:-?} ${h:-?}"
  case "${h:-}" in '' | *[!0-9]*) return 0 ;; esac
  # the plugin's view fills the window's bottom right
  left=$((x + w - VIEW_W))
  top=$((y + h - VIEW_H))
  cliclick "c:$((left + BEATBOX_X)),$((top + BEATBOX_Y))"
  sleep 3
  shot "$fmt-beatbox"
  gx=$((x + w - 8))
  gy=$((y + h - 8))
  cliclick "m:$gx,$gy" w:200 "dd:$gx,$gy" w:200 "dm:$((gx + 60)),$((gy + 40))" w:200 "dm:$((gx + 160)),$((gy + 100))" w:200 "du:$((gx + 160)),$((gy + 100))"
  sleep 3
  echo "$fmt window after the grip: $(bounds)"
  shot "$fmt-resized"
}

system_profiler SPDisplaysDataType SPAudioDataType 2>/dev/null | grep -vE '^\s*$' | head -40 || true
HOMEBREW_NO_AUTO_UPDATE=1 brew install --quiet cliclick

curl -fsSL -o "$work/reaper.dmg" "https://www.reaper.fm/files/7.x/reaper${version}_universal.dmg"
echo "$sha  $work/reaper.dmg" | shasum -a 256 -c -
yes | hdiutil attach -nobrowse -noverify -mountpoint "$work/mnt" "$work/reaper.dmg" >/dev/null || true
cp -R "$work/mnt/REAPER.app" "$work/"
hdiutil detach "$work/mnt" >/dev/null || true

config="$HOME/Library/Application Support/REAPER"
mkdir -p "$config/Scripts"
cp "$here/host-test/host_test.lua" "$config/Scripts/__startup.lua"

env -u GH_TOKEN CHIMERA_TEST_OUT="$out" CHIMERA_TEST_FORMATS=CLAP,VST3,AU CHIMERA_TEST_EDITOR=1 \
  "$work/REAPER.app/Contents/MacOS/REAPER" -nosplash >"$out/reaper.log" 2>&1 &
pid=$!

deadline=$((SECONDS + 600))
started=0
while [ ! -e "$out/finished" ]; do
  if [ "$SECONDS" -gt "$deadline" ]; then
    echo "::warning::REAPER didn't finish in time"
    break
  fi
  if ! kill -0 "$pid" 2>/dev/null; then
    echo "::warning::REAPER stopped by itself"
    break
  fi
  if [ "$started" = 0 ] && [ "$SECONDS" -gt 20 ]; then
    started=1
    shot start
  fi
  for marker in "$out"/at-*; do
    [ -e "$marker" ] || continue
    step=${marker##*/at-}
    rm "$marker"
    case "$step" in editor-*) editor "${step#editor-}" ;; esac
    touch "$out/go-$step"
  done
  sleep 0.5
done
shot end
kill "$pid" 2>/dev/null || true
sleep 2
kill -9 "$pid" 2>/dev/null || true

tail -20 "$out/reaper.log" || true
grep -il chimera "$config"/*.ini 2>/dev/null | while read -r f; do echo "--- $f"; grep -i -B1 -A1 chimera "$f" | cut -c1-160; done || true

bun "$here/host-test/check.ts" "$out"
