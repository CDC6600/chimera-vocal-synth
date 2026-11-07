#!/usr/bin/env bash
# Fetch the TSP speech database (P. Kabal, McGill University, v2 2018-11; CC BY 4.0 / Simplified
# BSD, "Copyright (c) 2018 Peter Kabal") at 48 kHz, for `chimera compare tsp`.
#   tools/tsp/fetch.sh [dir]     (default ~/datasets/tsp; the files land in <dir>/48k/<speaker>/)
set -euo pipefail
dir="${1:-$HOME/datasets/tsp}"
url="https://www.mmsp.ece.mcgill.ca/Documents/Data/TSP-Speech-Database/48k.zip"
sha="9cfb3a3a13014c8ff90770a5d1923f376da73ac927b9100f09826f60cf06cf43"
mkdir -p "$dir"
cd "$dir"
if ! echo "$sha  48k.zip" | sha256sum -c --status 2>/dev/null; then
  echo "downloading 48k.zip (254 MB) to $dir"
  curl -fL -C - -o 48k.zip "$url"
  echo "$sha  48k.zip" | sha256sum -c
fi
unzip -qo 48k.zip
echo "unpacked $(find 48k -name '*.wav' | wc -l) recordings to $dir/48k"
