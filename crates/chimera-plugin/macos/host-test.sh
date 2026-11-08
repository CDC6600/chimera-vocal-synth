#!/bin/bash
# CHIMERA in a real host: REAPER, on this Mac, loads the bundles that `build.sh --install` put in
# place, renders what each mode does (../host-test/host_test.lua, judged by ../host-test/check.ts),
# and opens each editor for this script to look at and click. GitHub's Macs have a display and let
# scripts capture it and post clicks, which is what makes this work on CI. See docs/plugin.md,
# "Testing in a host". Screenshots also go to the log: the workflow uploads artifacts before this runs.
set -euo pipefail
# from this file, not git: validate.sh runs it from a folder of downloaded tools
root=$(cd "$(dirname "$0")/../../.." && pwd)
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

# REAPER's first-run questions come before it runs any script. Answers one, and says what it asked.
dialogs() {
  osascript -e 'tell application "System Events" to tell process "REAPER"
    repeat with w in windows
      set names to name of buttons of w
      repeat with answer in {"No", "Still Evaluating", "Still evaluating", "I agree", "OK"}
        if names contains (answer as text) then
          set said to value of static texts of w
          click button (answer as text) of w
          return (answer as text) & " to: " & (said as text)
        end if
      end repeat
    end repeat
  end tell' 2>/dev/null || true
}

# Where the Beatbox tab is in the page at zoom 1, from a screenshot of it in WKWebView, and the
# height of REAPER's title bar and toolbar above the editor. The page zooms out below LAYOUT_MIN
# (page_zoom in crates/chimera-plugin/src/bridge.rs).
BEATBOX_X=612
BEATBOX_Y=26
CHROME=54
LAYOUT_W=1120
LAYOUT_H=800

about_reaper_open() {
  osascript -e 'tell application "System Events" to tell process "REAPER" to get name of every window' 2>/dev/null | grep -q 'About REAPER'
}

# REAPER's nag covers the window until its countdown lets dialogs() answer it
wait_for_nag() {
  local i answered
  for i in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20; do
    about_reaper_open || return 0
    answered=$(dialogs)
    [ -z "$answered" ] || echo "answered $answered"
    sleep 1
  done
}

editor() { # format
  local fmt=$1 x y w h view_h zoom tab_x tab_y gx gy
  wait_for_nag
  osascript -e 'tell application "System Events" to tell process "REAPER" to set frontmost to true' 2>&1 || true
  shot "$fmt-open"
  read -r x y w h <<<"$(bounds)" || true
  echo "$fmt window: ${x:-?} ${y:-?} ${w:-?} ${h:-?}"
  case "${h:-}" in '' | *[!0-9]*) return 0 ;; esac
  if [ "$fmt" = AU ]; then
    # REAPER's own toggle between the plugin's view and its generic sliders
    cliclick "c:$((x + w - 72)),$((y + 39))"
    sleep 3
    shot "$fmt-ui-button"
    return 0
  fi
  view_h=$((h - CHROME))
  zoom=$(awk -v w="$w" -v h="$view_h" -v lw="$LAYOUT_W" -v lh="$LAYOUT_H" 'BEGIN { z = w / lw; if (h / lh < z) z = h / lh; if (z > 1) z = 1; print z }')
  tab_x=$(awk -v x="$x" -v z="$zoom" -v t="$BEATBOX_X" 'BEGIN { printf "%d", x + z * t }')
  tab_y=$(awk -v y="$y" -v z="$zoom" -v c="$CHROME" -v t="$BEATBOX_Y" 'BEGIN { printf "%d", y + c + z * t }')
  echo "$fmt zoom $zoom, Beatbox tab at $tab_x,$tab_y"
  cliclick "c:$tab_x,$tab_y"
  sleep 3
  shot "$fmt-beatbox"
  gx=$((x + w - 5))
  gy=$((y + h - 5))
  cliclick "m:$gx,$gy" w:200 "dd:$gx,$gy" w:200 "dm:$((gx - 30)),$((gy - 20))" w:200 "dm:$((gx - 100)),$((gy - 60))" w:200 "du:$((gx - 100)),$((gy - 60))"
  sleep 3
  echo "$fmt window after the grip: $(bounds)"
  shot "$fmt-resized"
}

echo "screen: $(osascript -e 'tell application "Finder" to get bounds of window of desktop' 2>&1)"
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

deadline=$((SECONDS + 420))
started=0
first_shot=$((SECONDS + 60))
while [ ! -e "$out/finished" ]; do
  if [ "$SECONDS" -gt "$deadline" ]; then
    echo "::warning::REAPER didn't finish in time"
    break
  fi
  if ! kill -0 "$pid" 2>/dev/null; then
    echo "::warning::REAPER stopped by itself"
    break
  fi
  answered=$(dialogs)
  [ -z "$answered" ] || echo "answered $answered"
  if [ "$started" = 0 ] && [ "$SECONDS" -gt "$first_shot" ]; then
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

echo "--- the AU's view, loaded as Apple documents for hosts"
swift "$here/host-test/au_view.swift" 2>&1 | tail -20 || true

bun "$here/host-test/check.ts" "$out"
