#!/usr/bin/env bash
# Build CHIMERA.clap, CHIMERA.vst3 and CHIMERA.component on macOS, as universal binaries, into
# target/bundled. All three bundles hold the same dylib: it exports clap_entry, and clap-wrapper
# adds the VST3 and AUv2 entry points. The signature is ad hoc (no Developer ID yet).
#
#   crates/chimera-plugin/macos/build.sh           build
#   crates/chimera-plugin/macos/build.sh --install build, then copy into ~/Library/Audio/Plug-Ins
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"

version=$(sed -n '/^\[workspace.package\]/,/^\[/s/^version = "\(.*\)"/\1/p' Cargo.toml)
targets=(aarch64-apple-darwin x86_64-apple-darwin)
out=target/bundled
here=crates/chimera-plugin/macos

# The page the editor shows is embedded by build.rs, so it has to exist first.
bun install --frozen-lockfile
bun run build

libs=()
for target in "${targets[@]}"; do
  cargo build -p chimera-plugin --profile plugin --target "$target"
  libs+=("target/$target/plugin/libchimera_plugin.dylib")
done
lipo -create "${libs[@]}" -output target/libchimera_plugin.universal.dylib
lipo -info target/libchimera_plugin.universal.dylib

bundle() { # extension, Info.plist template
  local dir="$out/CHIMERA.$1"
  rm -rf "$dir"
  mkdir -p "$dir/Contents/MacOS"
  cp target/libchimera_plugin.universal.dylib "$dir/Contents/MacOS/CHIMERA"
  sed "s/@VERSION@/$version/g" "$here/$2" > "$dir/Contents/Info.plist"
  printf 'BNDL????' > "$dir/Contents/PkgInfo"
  codesign --force --sign - "$dir"
  plutil -lint "$dir/Contents/Info.plist"
}
mkdir -p "$out"
bundle clap Info.clap.plist
bundle vst3 Info.vst3.plist
bundle component Info.component.plist

if [ "${1:-}" = "--install" ]; then
  base="$HOME/Library/Audio/Plug-Ins"
  for pair in clap:CLAP vst3:VST3 component:Components; do
    mkdir -p "$base/${pair#*:}"
    rm -rf "$base/${pair#*:}/CHIMERA.${pair%:*}"
    cp -R "$out/CHIMERA.${pair%:*}" "$base/${pair#*:}/"
  done
  # the AU registry caches what it has seen
  killall -9 AudioComponentRegistrar 2>/dev/null || true
  echo "installed into $base"
fi
