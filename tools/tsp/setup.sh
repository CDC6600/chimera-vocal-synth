#!/usr/bin/env bash
# One-time setup for `chimera compare tsp`: the Montreal Forced Aligner (McAuliffe et al. 2017,
# MIT; v3.4.3 from conda-forge, through micromamba) and its english_us_arpa v3.0.0 acoustic model
# and dictionary (CC BY 4.0), in ~/.cache/chimera-mfa (about 2 GB). Pinned and checked: another
# model aligns differently, and the judge's numbers would move with it.
set -euo pipefail
cache="$HOME/.cache/chimera-mfa"
export MAMBA_ROOT_PREFIX="$cache/root" MFA_ROOT_DIR="$cache/mfa" PATH="$cache/env/bin:$PATH"
if [ ! -x "$cache/env/bin/mfa" ]; then
  mkdir -p "$cache"
  curl -fL -o "$cache/micromamba" https://github.com/mamba-org/micromamba-releases/releases/download/2.10.0-0/micromamba-linux-64
  chmod +x "$cache/micromamba"
  "$cache/micromamba" create -y -q -p "$cache/env" -c conda-forge montreal-forced-aligner=3.4.3 python=3.11
fi
have="$(mfa version 2>/dev/null || true)"
if [ "$have" != "3.4.3" ]; then
  echo "$cache/env has MFA ${have:-?}, not 3.4.3: remove $cache/env and run this again" >&2
  exit 1
fi
models="$cache/mfa/pretrained_models"
check() { echo "$1  $2" | sha256sum -c --status 2>/dev/null; }
acoustic=d35ce271ded357d833d2f4b8d1041dc3748b9538567ba13f2c697f4e4126711b
dictionary=e8c6c7b036ae2b7c78d2768b8dc6b1f9359175b842956d00b48c53c9c332e6b0
check "$acoustic" "$models/acoustic/english_us_arpa.zip" ||
  mfa model download acoustic english_us_arpa --version v3.0.0 --ignore_cache >/dev/null 2>&1
check "$dictionary" "$models/dictionary/english_us_arpa.dict" ||
  mfa model download dictionary english_us_arpa --version v3.0.0 --ignore_cache >/dev/null 2>&1
check "$acoustic" "$models/acoustic/english_us_arpa.zip" || { echo "english_us_arpa acoustic model: unexpected checksum" >&2; exit 1; }
check "$dictionary" "$models/dictionary/english_us_arpa.dict" || { echo "english_us_arpa dictionary: unexpected checksum" >&2; exit 1; }
echo "MFA $(mfa version) and english_us_arpa v3.0.0 ready in $cache"
