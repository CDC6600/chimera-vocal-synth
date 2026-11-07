# Comparing the synth's speech to real voices: the TSP speech database

`chimera compare tsp` says the Harvard sentences that 23 real speakers read and measures each
render against the recording, phone by phone. Whisper's word error rate
([tools/asr/speech_wer.py](../tools/asr/speech_wer.py)) says whether a sentence is understood.
This judge says which sounds differ from real speech, and by how much: how loud a /ʃ/ is next to
the vowels, where its noise sits, how long a /k/ closure lasts.

## The dataset

TSP speech database (P. Kabal, McGill University, version 2, 2018;
[report](https://www.mmsp.ece.mcgill.ca/Documents/Data/TSP-Speech-Database/TSP-Speech-Database.pdf)):

- 12 female and 11 male adult speakers, mostly Canadian English
- each reads 6 of the 72 Harvard lists (60 sentences)
- 48 kHz, recorded in an anechoic room with the microphone about 15 cm away, at a constant gain

The database is under a Simplified BSD licence ("Copyright (c) 2018 Peter Kabal"; the report
itself is CC BY 4.0). Redistribution and derivatives are allowed with the notice, so the
sentences and alignment derived from it are in git, with the notice in
[tools/tsp/README.md](../tools/tsp/README.md). The audio is fetched rather than committed.

```sh
tools/tsp/fetch.sh            # 254 MB to ~/datasets/tsp; unpacks 48k/<speaker>/*.wav
tools/tsp/setup.sh            # once: the Montreal Forced Aligner and its pinned model, ~2 GB in ~/.cache
```

- **Children.** The two child speakers (CA, CB) are left out.
- **Wrong recordings.** Four recordings say another sentence than the one in their header, and
  are left out (`EXCLUDE` in `tools/tsp/align.py`). `tools/tsp/check_text.py` found them by
  transcribing every recording with Whisper.
- **Whisper on the real voices.** Whisper gets 2.7 % of the words wrong over all 1378 adult
  recordings (2.9 % female, 2.5 % male). It gets about 37 % wrong on the synth's sentences.
  1374 recordings remain.

## Running it

```sh
cargo run --release -p chimera-cli -- compare tsp                         # all 1374, ~2 min on 16 cores
cargo run --release -p chimera-cli -- compare tsp ~/datasets/tsp out/tsp-new --against out/tsp/score.json
#   --speakers FA,MA   only these speakers (no verdict across different sets)   --jobs n   --dump
```

It writes the following to the output directory (`out/tsp` by default):

- `report.md`: the score, the score a real speaker gets, the groups per sex, the scored terms by
  cost, the worst paired tokens (listening targets), and the diagnostics by cost.
- `score.json`: everything the verdict needs. For every term, the speakers' centre and spread,
  the synth's value, the cost, and the number of synth tokens behind it.
- `mfa/`: the renders, the words they say, and how MFA aligned them.
- With `--dump`: `<voice>-<list>_<n>.wav` and the plan's phones, for listening beside
  `~/datasets/tsp/48k/<speaker>/<speaker><list>_<n>.wav`. The licence allows putting both on a
  listening page.

## Method

**What the synth says.** Each sentence is said the way `chimera render text` says it: CMUdict
G2P, the phrase planner, 112 Hz (male) or 205 Hz (female). The female body reads the women's
sentences and the male body the men's. The speech judge's renders and these come from the same
code (`render::say`).

**Phone timing, the same for both voices.** The Montreal Forced Aligner (MFA 3.4.3,
`english_us_arpa` v3.0.0) times the phones of both the recordings and the renders. It uses the
same settings for both (`tools/tsp/mfa.yaml`).

- **Recordings.** Aligned once with MFA's dictionary, which lists pronunciation variants, so a
  speaker's "the" can be ðə or ði. The result is committed (`tools/tsp/alignment.tsv.gz`);
  `tools/tsp/align.sh` regenerates it.
- **Renders.** Aligned on every run (about 25 s) with a dictionary of exactly what the G2P said,
  including its reduced function words: "of" is AH0 V.
- **Why not the planner's own times.** The planner's segments are gestures, not sounds. Over
  120 renders, MFA put the synth's vowels 29 ms shorter than the plan's segments (median; the
  start 22 ms later and the end 12 ms earlier). It put voiceless fricatives 29 ms longer and
  nasals 24 ms longer, and gave a stop's aspiration to the stop. Taking the synth's times from
  the plan and the speakers' from MFA would make the synth's vowels look 30 ms longer than they
  sound.
- **Reproducible.** By default MFA adds random dither to its features and adapts to each
  speaker over all of that speaker's files. Then the same file aligns differently from run to
  run (65 of 120 renders), and a change in one sentence could move another sentence's
  alignment. `mfa.yaml` turns both off, and every utterance is its own "speaker". Two runs are
  identical, and each file's alignment depends on that file alone.
- **The price.** Without speaker adaptation, MFA is a little less accurate on the real voices.
  Compared with an adapted alignment, 25 % of the boundaries move, by 10 ms at the 90th
  percentile and 50 ms at the 99th.

**Matching.** A Levenshtein alignment of the recording's phones against the render's keeps the
pairs that are the same phone. The pair takes the synth's stress for both voices. A phone one
voice says and the other doesn't is left out of both. About 14,000 female and 13,000 male
phones match in the training sentences. None of the 32,766 matched pairs falls in a different
word of the two sentences, so the matching isn't constrained by word.

**What is measured.** Each matched phone is measured by the same code for both voices. A phone's
"middle" is its middle half, at least its centre 10 ms.

| measure | phones | how |
|---|---|---|
| level | consonants | RMS (dB) of the middle (an affricate: its second half, the frication), noise floor subtracted, re the utterance's stressed vowels; left out within 3 dB of the floor |
| burst | stops | loudest 5 ms from a third into the stop to its end (MFA gives the release and aspiration to the stop), re the stressed vowels |
| centroid | /s z ʃ tʃ dʒ/ | power-weighted mean frequency, 1–10 kHz, over the middle (affricates: their second half) |
| duration | every phone | over the utterance's mean phone duration, so speaking rate cancels |
| spectrum | each consonant and monophthong | power averaged over the middle's frames, as a log-mel spectrum (40 bands, 60 Hz–10 kHz), the logs averaged over tokens, as 13 cepstral coefficients without c0 |
| F1, F2 | stressed monophthongs | LPC at 35, 50 and 65 % of the vowel (12 kHz copy, order 12 female / 14 male) |
| vowel bands, H1−H2 | stressed vowels | the singer judge's band levels (0–0.5 … 4–8 kHz re 0–8 kHz), and first minus second harmonic |
| voiced | each consonant; vowels | share of the middle's 10 ms frames that YIN voices, on the band below 1.2 kHz, so a voiced fricative reads as voiced |

Per utterance it also measures the articulation rate (phones per second of phone time) and the
f0 range (10th to 90th percentile of voiced frames, in semitones).

## What is scored

The score holds only the measures checked against the data and against what the owner heard.
The rest are diagnostics: reported with costs on the same scale, never scored. A diagnostic can
join the score once it is shown to rank changes the way a listener does.

- **level:** each consonant's level, each stop's burst, the loudest tenth of all bursts, and
  the bursts of utterance-final stops.
- **sibilants:** the centroids of /s z ʃ tʃ dʒ/. Batch 2's /ʃ/ fix moved these toward the
  speakers, and the owner heard "sheet" come back.
- **timing:**
  - each consonant's duration
  - stressed vowels by what follows: a voiceless obstruent, a voiced one, a sonorant, the end
  - the shortest tenth of stressed vowels
  - unstressed vowels
  - the articulation rate
- **Diagnostics:** spectrum (MCD), formants, voice (vowel bands, H1−H2), voicing, intonation (f0
  range).

**Why the voice terms aren't scored.** They disagree with the owner's ear. Batch 1 darkened the
voice, and the owner preferred it 7 to 0. These terms call it worse: it fixed the female 4–8 kHz
band but cost the 1–4 kHz presence real voices have (below). That may be the recording chain,
or a real shape error the owner didn't weigh. Until it is settled, they only advise.

**Why the others aren't scored:**

- **Formants and spectra.** These are estimator agreement as much as articulation: LPC can lock
  onto harmonics, and a spectrum carries the microphone.
- **Voicing.** It rests on one YIN threshold.
- **Intonation.** An f0 range can be matched by wobble.

**Pops.** A median doesn't see one bad token. Pops and clicks stay with
`crates/chimera/tests/consonant_artifacts.rs`. The report lists the worst paired tokens: the
stressed vowels shortest against the speaker's same vowel, and the bursts loudest against the
speaker's same stop. Those are the ones to listen to.

## The score

The score is how far the synth is from the real speakers, in units of how much they differ from
each other. It is built like the singer judge's ([csd-comparison.md](csd-comparison.md)).

- **Speakers.** The training speakers are A–J (10 female, 9 male; lists 1–60). The held-out
  speakers are K and L (2 and 2; lists 61–72), as the TSP report suggests.
- **A voice's value on a term.** The median over its tokens (a mean for voiced shares, a
  percentile for the "tenth" terms), with at least 3 tokens. A speaker gives one value per term
  from their 60 sentences.
- **The synth's value.** The synth gets a value on each training speaker's sentences, and its
  value on a term is the median over those, from at least half of them. So the synth and the
  speakers give the same kind of number, each from the same lists.
- **Centre.** A term's centre is the median over that sex's training speakers, from at least 5.
- **Spread.** A term's spread is the MAD, scaled to a standard deviation, of each training
  speaker's signed deviation from their sex's centre. It is pooled over both sexes (19
  speakers), so that it isn't small by chance. Frequencies are on a log scale, so their spreads
  pool. `score.json` also gives each sex's own spread.
- **Spread floor.** A spread is at least about a just-noticeable difference:
  - 1.5 dB for levels and bursts
  - 1 dB for vowel bands and H1−H2
  - 5 % for frequencies
  - 0.1 for relative durations and voiced shares
  - 0.5 phones/s for the articulation rate
  - 1 semitone for the f0 range

  Without the floor, a measure every speaker happens to share (no voicing at all in /s/: a
  spread of 0.01) would turn any difference into a large cost.
- **Cost.** A term's cost is max(0, |synth − centre| / spread − 1). Within one spread of the
  real speakers costs nothing. A term with no value has no cost, never a cost of 0. A scored
  group with no cost leaves the score empty: a run over a few speakers has no centres, so no
  score.
- **Spectrum terms (diagnostic).** These are distances. The synth's cepstrum is the mean over
  the speakers' sentence sets, compared with the centre (the mean of the training speakers'
  cepstra), in MCD dB. The unit is the median, over training speakers, of a speaker's distance
  to the centre of the others: how far apart two real voices are on that phone. Cost =
  max(0, distance / unit − 1).
- **Groups.** Each scored group gets one share. A sex's score is the mean of its three group
  means, and the score is the mean over the two sexes.
- **A real speaker's score.** Each training speaker is scored the same way against the other
  training speakers; the report gives the median. That is what "as close as a person" means on
  this scale. The held-out speakers are scored against the training speakers.
- **Held-out.** The synth on the held-out speakers' sentences (lists 61–72), against the
  training centres and spreads. It shows whether a gain carries over to sentences it wasn't
  tuned on.

**The verdict.** `--against <earlier score.json>` compares the training scores of two runs. It
refuses unless both used the same judge version, the same alignment of the recordings, the same
aligner (MFA's version, its model and `mfa.yaml`) and the same recordings, by ID. The synth and the aligner are deterministic, so the same
build gives the same score (checked: a second run is identical, and its verdict is NO CLEAR
CHANGE, +0.000). Only scored terms count:

- **Better:** the score falls by at least 0.01, and no scored term breaks.
- **A term breaks** when its cost rises by more than 0.5, when it loses its value, or when it
  rests on 5 % fewer synth tokens than before. A change can't improve a term by losing the
  tokens it does worst on.
- **Worse:** a term breaks, or the score rises by 0.01 or more.
- **No clear change:** anything else.

These thresholds are policy, not measured audibility. A batch of many experiments will break a
term even while the score falls a lot: batch 2 as a whole comes out WORSE (−0.214, with seven
terms up by 0.5–1.0).

## Caveats

- **Microphone and room.** The speakers were recorded 15 cm from a real microphone, which
  lifts the bass of a directional microphone (the proximity effect); the synth is dry. Their
  noise floor is 28–50 dB below their vowels, so the weakest fricatives (/f θ/) of the
  noisiest speakers sit near it. Levels subtract the noise floor; spectra don't. The TSP report
  notes some breath pops in the recordings. They are not a target.
- **The aligner.** A boundary MFA misplaces moves the measurement window. The middle-half
  windows tolerate errors of about a quarter of the phone. Bursts and durations are the most
  exposed. Some of the shortest-vowel tokens in the report are likely the aligner failing on
  the synth rather than a squeezed vowel. Listen before acting on one.
- **One synth voice per sex, many speakers.** The synth is compared with the range of real
  speakers, not with any one of them. A term within the range costs nothing, whatever its
  distance from the centre.
- **Accent.** The speakers are mostly Canadian; the G2P is General American (CMUdict). Matching
  drops the phones they say differently.

## Baseline (2026-11-07, after batch 2)

**Score 1.583 on the training speakers' sentences, 1.706 on the held-out speakers'.** A real
speaker gets 0.277 (median over the training speakers; 0.242 for the held-out ones). Per sex:
female 2.19, male 0.97.

| group | female | male |
|---|---|---|
| level | 1.80 | 0.68 |
| sibilants | 3.82 | 1.38 |
| timing | 0.96 | 0.85 |
| *spectrum* | *1.33* | *1.64* |
| *formants* | *1.26* | *1.04* |
| *voice* | *0.65* | *1.16* |
| *voicing* | *0.35* | *0.46* |
| *intonation* | *1.26* | *0.86* |

(Diagnostics in italics.) What it points at, largest first:

1. **The female sibilants are too quiet and too low.**

   | | women | synth |
   |---|---|---|
   | /tʃ/ level (frication) | −9 dB re her stressed vowels | −26 dB |
   | /ʃ/ level | −7 dB | −23 dB |
   | /s/ level | −11 dB | −21 dB |
   | /s/ centroid | 7.4 kHz | 4.7 kHz |
   | /z/ centroid | 7.1 kHz | 4.0 kHz |
   | /ʃ/ centroid | 4.0 kHz | 3.3 kHz |

   The male /tʃ/ is 8 dB quiet and his /ʃ/ 6 dB. His /s z/ centroids are 1.2–1.5 kHz low.
   The female /h/ is 14 dB quiet.
2. **Closures and affricates are short; nasals long.**

   | | speakers | synth |
   |---|---|---|
   | /k/ | 1.1–1.2 of a mean phone | 0.7–0.8 |
   | /dʒ/ | 1.2–1.3 | 0.5 |
   | /z/ | 0.9–1.0 | 0.5–0.6 |
   | /ŋ/ | 0.9–1.0 | 1.3–1.4 |

   Stressed vowels before sonorants are long (female 1.49 against 1.19).
3. **Bursts.** Female /t k/ bursts are 9 dB weak. /ð/ and /v/ are 7–11 dB too loud in both voices.
4. **Diagnostics worth a look.**
   - **The voice lacks presence at 1–4 kHz.** Over the stressed vowels, in dB re 0–8 kHz: at
     1–2 kHz, women −10.8, female synth −16.1, men −10.6, male synth −19.7; at 2–4 kHz, −16.4,
     −22.5, −18.6, −26.4. The male synth still has too much above 4 kHz (−29.6, men −35.0).
     Before batch 1, the female 1–2 and 2–4 kHz bands were −13.2 and −15.5.
   - **/u/ is too far back.** F2 is 1160 Hz (women 1570) and 1070 Hz (men 1560).
   - **/z/ and /ð/ are too voiced.** The speakers voice about a third of their frames, the synth
     two thirds or more.

### What the batches did, on this judge

| commit | training | held-out | female | male |
|---|---|---|---|---|
| before batch 1 (5186d64) | 1.994 | 2.063 | 2.86 | 1.13 |
| after batch 1 (4980e3b) | 1.797 | 1.888 | 2.59 | 1.00 |
| after batch 2 (now) | 1.583 | 1.706 | 2.19 | 0.97 |

- **Batch 1** mostly helped timing (female 1.28 → 0.96, male 1.07 → 0.93).
- **Batch 2** moved the female /ʃ/ centroid from 2.8 to 3.3 kHz (women 4.0). It overshot the
  male /ʃ/ (3.27 → 3.48 kHz, men 3.13). No batch moved the female sibilant levels.
