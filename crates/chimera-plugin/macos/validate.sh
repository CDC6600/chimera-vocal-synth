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
auval -a | grep -i chimera || { echo "the AU registry doesn't list CHIMERA"; exit 1; }
auval -v aumu Voic Chma
echo "::endgroup::"

echo "::group::pluginval (VST3)"
gh release download -R Tracktion/pluginval -p 'pluginval_macOS.zip'
unzip -q pluginval_macOS.zip
# --skip-gui-tests: a runner has no display session to open the editor in
./pluginval.app/Contents/MacOS/pluginval --strictness-level 5 --skip-gui-tests --validate "$bundled/CHIMERA.vst3"
echo "::endgroup::"
