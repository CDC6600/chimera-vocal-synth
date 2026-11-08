#!/usr/bin/env bash
# Install CI's newest build of the plugin into ~/Library/Audio/Plug-Ins, with nothing to download or
# move by hand. Needs `gh`, logged in, because GitHub wants an account to download a run's artifacts
# even from a public repo: brew install gh && gh auth login
#
#   update.sh           the plugin branch's newest build that passed
#   update.sh <branch>  another branch's
set -euo pipefail

repo=jamesondh/chimera-vocal-synth
branch=${1:-t3/scope-plugin-formats}
base="$HOME/Library/Audio/Plug-Ins"
stamp="$HOME/Library/Caches/chimera-plugin-commit"

command -v gh >/dev/null || { echo "This needs GitHub's gh: brew install gh && gh auth login" >&2; exit 1; }
gh auth status >/dev/null 2>&1 || { echo "Log in to GitHub first: gh auth login" >&2; exit 1; }

latest() { # extra `gh run list` arguments: prints "run commit title"
  gh run list -R "$repo" --workflow plugin.yml --branch "$branch" --limit 1 "$@" \
    --json databaseId,headSha,displayTitle --jq '.[0] // empty | "\(.databaseId) \(.headSha) \(.displayTitle)"'
}

read -r run sha title <<<"$(latest --status success)"
[ -n "${run:-}" ] || { echo "No finished build on $branch." >&2; exit 1; }

read -r _ newest _ <<<"$(latest)"
[ "${newest:-$sha}" = "$sha" ] || echo "A newer build (${newest:0:7}) isn't finished or didn't pass; installing the last good one."

if [ -d "$base/VST3/CHIMERA.vst3" ] && [ "$(cat "$stamp" 2>/dev/null)" = "$sha" ]; then
  echo "Already on the newest build: ${sha:0:7} $title"
  exit 0
fi

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
echo "Downloading ${sha:0:7} $title"
gh run download "$run" -R "$repo" -n chimera-plugin-macos -D "$tmp"

# unpack all three before touching what's installed, so a bad download leaves it as it was
for ext in clap vst3 component; do
  ditto -x -k "$tmp/CHIMERA.$ext.zip" "$tmp/unpacked"
  [ -x "$tmp/unpacked/CHIMERA.$ext/Contents/MacOS/CHIMERA" ] || { echo "CHIMERA.$ext isn't complete in this build." >&2; exit 1; }
done

for pair in clap:CLAP vst3:VST3 component:Components; do
  ext=${pair%:*} dir=$base/${pair#*:}
  mkdir -p "$dir"
  # remove first, never overwrite: a host may have the old binary loaded, and changing its file in
  # place would crash it, while a removed one stays valid until the host lets go of it
  rm -rf "$dir/CHIMERA.$ext"
  ditto "$tmp/unpacked/CHIMERA.$ext" "$dir/CHIMERA.$ext"
  xattr -dr com.apple.quarantine "$dir/CHIMERA.$ext" 2>/dev/null || true
done

# the AU registry caches what it has seen
killall -9 AudioComponentRegistrar 2>/dev/null || true
mkdir -p "$(dirname "$stamp")"
echo "$sha" >"$stamp"
echo "Installed ${sha:0:7}. Restart your DAW to load it: one that's running keeps the old version."
