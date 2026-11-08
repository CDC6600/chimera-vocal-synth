#!/usr/bin/env bash
# Validate the bundles from build.sh --install with each format's own validator. CI runs this; it
# needs `gh`, logged in, to fetch the validators.
set -euo pipefail
root=$(git rev-parse --show-toplevel)
bundled="$root/target/bundled"
tools=$(mktemp -d)
cd "$tools"

echo "::group::clap-validator"
gh release download -R free-audio/clap-validator -p '*macos-universal.zip'
unzip -q ./*.zip
for t in ./*.tar.gz; do [ -e "$t" ] && tar xzf "$t"; done
validator=$(find . -type f -name clap-validator -perm -u+x | head -1)
# excluded: failures nih-plug's own examples share (state reload with this validator version), and
# sample rates below 32 kHz, where the plugin is silent on purpose (the airway goes unstable)
"$validator" validate --only-failed \
  -x 'state-reproducibility-.*' -x state-invalid-random -x process-varying-sample-rates \
  "$bundled/CHIMERA.clap"
echo "::endgroup::"

echo "::group::auval"
# the registry takes a moment to notice a new component
for _ in 1 2 3 4 5 6 7 8 9 10; do
  auval -a | grep -q 'Voic Chma' && break
  sleep 3
done
auval -a | grep 'Voic Chma' || { echo "the AU registry doesn't list CHIMERA"; exit 1; }
auval -v aumu Voic Chma
echo "::endgroup::"

echo "::group::pluginval (VST3)"
gh release download -R Tracktion/pluginval -p 'pluginval_macOS.zip'
unzip -q pluginval_macOS.zip
# --skip-gui-tests: a runner has no display session to open the editor in
./pluginval.app/Contents/MacOS/pluginval --strictness-level 5 --skip-gui-tests --validate "$bundled/CHIMERA.vst3"
echo "::endgroup::"

echo "::group::REAPER"
# doesn't fail the run yet: update.sh installs the newest passing build, and this is still new
"$root/crates/chimera-plugin/macos/host-test.sh" || echo "::warning::the REAPER test failed: see this step's log"
echo "::endgroup::"
