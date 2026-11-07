# TSP: the real speakers for `chimera compare tsp`

See [docs/speech-comparison.md](../../docs/speech-comparison.md) for the judge.

| file | what |
|---|---|
| `fetch.sh` | downloads the TSP speech database's 48 kHz files to `~/datasets/tsp` |
| `setup.sh` | installs the Montreal Forced Aligner and its pinned model in `~/.cache/chimera-mfa` |
| `align.sh`, `align.py`, `mfa.yaml` | regenerate `sentences.tsv` and `alignment.tsv.gz` |
| `check_text.py` | Whisper's check that each recording says its sentence |
| `sentences.tsv` | recording, speaker, sentence (from each WAV's header), adults only, less 4 wrong recordings |
| `alignment.tsv.gz` | recording, tier (word, phone), start, end (s), label (ARPAbet with stress) |

## Versions

- **TSP speech database:** version 2 (2018-11), `48k.zip`, sha256
  `9cfb3a3a13014c8ff90770a5d1923f376da73ac927b9100f09826f60cf06cf43`.
- **Aligner:** Montreal Forced Aligner 3.4.3 (conda-forge).
- **Acoustic model:** `english_us_arpa` v3.0.0, a GMM-HMM trained 2024-02-10 on 2485 speakers,
  sha256 `d35ce271ded357d833d2f4b8d1041dc3748b9538567ba13f2c697f4e4126711b`.
- **Dictionary:** `english_us_arpa` v3.0.0, sha256
  `e8c6c7b036ae2b7c78d2768b8dc6b1f9359175b842956d00b48c53c9c332e6b0`.
- **Settings:** `mfa.yaml`. No dither and no speaker adaptation, and every recording is its own
  speaker, so each alignment is reproducible and independent of the other files.

## Licences

The TSP speech database. Its report says the database is made available under the Simplified
BSD licence below. The report itself is CC BY 4.0. `sentences.tsv` and `alignment.tsv.gz` are
derived from the database.

> Copyright (c) 2018 Peter Kabal
> All rights reserved.
>
> Redistribution and use in source and binary forms, with or without modification, are
> permitted provided that the following conditions are met:
>
> 1. Redistributions of source code must retain the above copyright notice, this list of
>    conditions and the following disclaimer.
> 2. Redistributions in binary form must reproduce the above copyright notice, this list of
>    conditions and the following disclaimer in the documentation and/or other materials
>    provided with the distribution.
>
> THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR
> IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY
> AND FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR
> CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR
> CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
> SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
> THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR
> OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE
> POSSIBILITY OF SUCH DAMAGE.

The Harvard sentences are from the IEEE Recommended Practice for Speech Quality Measurements
(1969). The Montreal Forced Aligner is MIT-licensed. Its `english_us_arpa` model and dictionary
are CC BY 4.0 (McAuliffe, Socolof, Mihuc, Wagner & Sonderegger, Interspeech 2017). Neither is
redistributed here: `setup.sh` fetches them.
