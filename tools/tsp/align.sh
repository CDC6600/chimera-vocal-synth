#!/usr/bin/env bash
# Regenerate tools/tsp/sentences.tsv and alignment.tsv: align the TSP recordings with the Montreal
# Forced Aligner (McAuliffe et al. 2017; english_us_arpa acoustic model and dictionary, CC BY 4.0).
# Only needed if the alignment changes: the judge reads the committed TSV for the recordings and
# aligns only its own renders, with the same model and settings.
# Installs MFA on first use (tools/tsp/setup.sh). Settings: tools/tsp/mfa.yaml.
#   tools/tsp/align.sh [tsp-dir]     (default ~/datasets/tsp, from tools/tsp/fetch.sh)
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
tsp="${1:-$HOME/datasets/tsp}"
"$here/setup.sh"
cache="$HOME/.cache/chimera-mfa"
export MAMBA_ROOT_PREFIX="$cache/root" MFA_ROOT_DIR="$cache/mfa" PATH="$cache/env/bin:$PATH"
mfa="$cache/env/bin/mfa"
py="$cache/env/bin/python"
rm -rf "$tsp/mfa/corpus"
"$py" -I "$here/align.py" prepare "$tsp" "$tsp/mfa/corpus"
"$mfa" align -q --clean -c "$here/mfa.yaml" -j "$(nproc)" --output_format json \
  "$tsp/mfa/corpus" english_us_arpa english_us_arpa "$tsp/mfa/aligned"
"$py" -I "$here/align.py" collect "$tsp/mfa/aligned"
