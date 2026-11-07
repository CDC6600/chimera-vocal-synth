# Hill-climb log

The rules are in [README.md](README.md). Entries are in order, newest last.

## Baseline (2026-11-07)

- **Singer:** training score 1.433, held-out 1.590. 0 means every term is within her own
  variation; lower is better.
- **Speech:** word error rate 38.7 %. Harvard sentences 32.5 % (male) and 37.5 % (female);
  nonsense sentences 47.9 % and 38.0 %.
- **Speed:** the voice takes 18.3 % of one core in real time (`os=2`).

## Leads

The largest costs at the baseline, and what they point at. When an experiment closes a lead or
opens a new one, update this list and give the experiment's number.

1. **Spectral tilt and the fundamental.** Costs: level 4–8 kHz 5.4, H1−H2 2.1, level 2–4 kHz
   1.4, level 0–0.5 kHz 1.4.
   - Too much energy above 2 kHz and too little below 500 Hz, on every vowel.
   - H1−H2 is too low on low notes (−5.8 dB, she has +4.0) and too high on high ones (+8.2 dB,
     she has +0.1).
   - Together they sound like a source that is too pressed. Look at the open quotient, skew and
     closure sharpness, and at how they vary with pitch and effort
     (`crates/chimera/src/sources/glottis.rs`, and how `voice/human_voice.rs` drives it).
   - Experiment 2 softened the closure: the 2–4 and 4–8 kHz costs fell to 0.13 and 0.68, so the
     high bands are about done. H1−H2 (2.21) and level 0–0.5 kHz (1.32) did not move: they are
     not the closure. They point at the open quotient and how it follows pitch: the synth's
     H1−H2 rises with pitch where hers falls.
   - Experiment 3 opened a small posterior chink: level 0–0.5 kHz 1.32 → 1.14, H1−H2 only
     2.21 → 2.14. What is left of H1−H2 is how it follows pitch and vowel, not its level. By
     vowel class and pitch, the synth's H1−H2 swings two to three times as far as hers as the
     harmonics cross F1: close vowels −9 → +23 dB from MIDI 56 to 68 (hers 1 → 11), mid vowels
     −10 → +14 from 60 to 72 (hers 5 → −3). The tract's closed-glottis F1 bandwidths are 35–70 Hz
     (female body). Two candidates: a wider F1 bandwidth (wall resistance: doubling
     `WALL_SOFT.resistance` raises B1 by about half but lowers F1 by 10–20 Hz, so the vowel tables
     need `chimera calibrate`), or F1 that follows pitch (singers open the jaw on high notes).
2. **Aperiodicity.** Cost 2.9: the synth's is about twice hers. Look at jitter, shimmer, drift
   and aspiration noise in the source. She is a clean, trained voice.
   - Experiment 1 lowered the jitter: cost 1.74. Most of what is left comes from a few songs with
     voicing trouble (en043a 0.05, en018a 0.03), not from the source's median: with jitter and
     shimmer at zero the cost stays at 1.30. Lowering shimmer raises note MCD past its guard.
3. **Phrase onsets.** Costs: vowel-initial 1.4, voiceless-initial 1.1.
   - Voicing starts about 20 ms early before a vowel.
   - Before /s/, /f/ and /h/ the synth voices 100–130 ms before the note, so the voicing runs
     into the fricative or comes before it.
   - Look at how a phrase starts: the 60 ms lead in `plan_song`, and the glottal abduction of a
     voiceless onset.
   - Experiment 4 held the folds open from the in-breath to the first voiced segment: WORSE. With
     the glottis open, the breath (which reaches full pressure 20 ms before the first segment)
     blows a loud [h] before the vowel or the onset consonant, and the detector reads it as voicing
     100–150 ms early. The cause is upstream: the breath arrives before the first articulation is
     in place. Next angle: start the breath ramp with the first segment's closure or constriction
     (`breath` gesture in `plan_utterance`), not 60 ms ahead of it, and leave the folds alone.
   - Experiment 10 timed a sung phrase's breath to reach full drive as its first segment starts
     (20 ms later than before): vowel-initial cost 1.49 → 0.97 (median −0.016 → +0.004 s, hers
     +0.005), voiceless-initial 1.08 → 0.52 (−0.020 → +0.037 s, hers +0.018). The medians are
     about right now; what is left is the mean gap, so a few phrases far off (the /s/, /f/, /h/
     onsets voiced well before the note, and the phrase-initial stops that never phonate, lead 5).
4. **Vibrato.** Costs: rate 0.67, share 0.62. The synth puts 35-cent vibrato on every long note
   at 5.5 Hz. She uses it on about two thirds of her long notes, at about 6.1 Hz
   (`vibrato_gesture` in `crates/chimera/src/phonetics/sing.rs`).
   - Experiment 5 raised the rate to 5.9–6.1 Hz: rate cost 0.66 → 0, score −0.044, but WORSE on
     the fewer-notes guard (barely voiced short notes, lead 5). Retry it once lead 5 is fixed.
   - The like-for-like judge (after experiment 6) and experiment 8 have both cleared the way: retry
     it as it was. Rate cost is 0.64 at experiment 8.
   - Experiment 9 kept 6.0 Hz: rate cost 0.64 → 0. What is left is the share (0.65): the synth puts
     vibrato on every long note, she on about two thirds. That is a planner choice
     (`vibrato_gesture`), not the source's.
5. **Voicing in held notes.** Cost 0.26. Gaps mid-note, worst in en018a, en041a and en043a
   (0.65–0.70 voiced).
   - The same fault blocks other experiments (1, 3, 5): short notes on a stop coda that the synth
     voices 0–12 % of the time, so one stray voiced frame decides whether H1−H2 and pitch are
     measured. Any change to the sound flips a few of them either way, and the fewer-notes guard
     calls a loss WORSE. Fixing these notes is the most useful next step.
   - Experiment 6 found the main cause: 315 of the 368 notes the synth voices under 30 % (she
     voices them over 70 %) are a phrase's last note. `plan_song` ends a phrase's last vowel up to
     120 ms (30 % of the note) before the note's end to leave a breath, though callers already
     split phrases at rests. Holding the note to 50 ms before its end: score 0.889 → 0.747, voiced
     share and aperiodicity costs to about 0. WORSE only on the fewer-notes guard, through two
     phrase-initial notes the synth never voices (below). Retry it once those voice.
   - The note that blocks it, en044a no. 145 "pæ" at 63.77 s (phrase-initial, 0.18 s, then
     "dwi"): the /p/ burst and aspiration are there (63.76–63.84 s), then 50 ms at −55 dB, about
     25 dB below the aspiration, where the vowel should be, and voicing starts only with the next
     note's /d/. The vowel never phonates after a phrase-initial voiceless stop. Its old "onset"
     was a stray frame 80 ms before the note. en036a no. 71 "ki" is the same shape. Look at why the
     folds don't start after the stop's abduction at a phrase start (lead 3's breath timing, or the
     abduction gesture's release).
   - Experiment 7 retried experiment 6 at r = 0.05 under the like-for-like judge: BETTER (0.889 →
     0.747), speech and speed OK, but reverted on a test. `songs_match_typescript` in
     `crates/chimera/tests/golden_planning.rs` checks `plan_song` against TypeScript-era plans in
     `tests/golden/planning.json`, which `CHIMERA_BLESS` doesn't regenerate, so it fails on any
     change to how songs are planned. The change is ready to keep once the owner decides how that
     fixture may be updated (a bless path for the song plans, or an exception in the rules like the
     airway one). Until then, no `plan_song` change can be kept.
   - Experiment 8 kept it, once the planners' snapshots were blessed: voiced share cost 0.26 →
     0.04, aperiodicity 0.90 → 0.03. This lead is about closed. What is left is the phrase-initial
     notes that never phonate after a voiceless stop (en044a no. 145, en036a no. 71), which is
     lead 3's.
6. **Speech consonants.** Not scored, only guarded. Whisper mishears /tʃ/, /dʒ/, /ʃ/ and /s/:
   "sheet" as "feet", "chicken leg" as "quickly like", "juice" as "roost". This is for a later
   loop on speech.
   - Experiment 11 found why /ʃ/ is heard as /f/: the chamber in front of the raised tongue tip
     (under the alveolar ridge, and the sublingual space) took the mid-oral area law, ~5.5 cm²,
     so the front-cavity resonance that the teeth source excites sat at 1.7 kHz (male) and
     2.1 kHz (female). A narrower law for the rays ahead of the tip: speech 40.73 → 35.76 %
     (BETTER), singer guards pass, but reverted on `geometry_continuity` (an existing jump at
     the top of the pharynx that the recalibrated child /ə/ lands on). Fix that jump, then keep
     experiment 11 as it was. The /ʃ/ is still about 21 dB below the vowel (female), as loud as
     /f/: its level is the next thing to look at.
   - Experiment 12 kept it (α = 1.6), together with the fix for that jump: the roof-contact
     model's shortcut ignored how far a ray ends off the roof (the velopharyngeal port window).
     Speech 40.73 → 36.09 %. "Sheet" is now heard in the male voice. Still failing: the female
     "sheet", chair, chain, birch and chopped. Next: the /ʃ/'s level (teeth source gain, or
     `w_dist` near 2.5 cm), then α = 1.2.
   - Experiment 13 found `w_dist` is not the cause (both bodies' /ʃ/ get full weight), and that
     in both voices /ʃ/ is no louder than /f/, where real sibilants are about 10 dB louder.
     Doubling the teeth (obstacle) source makes the female "sheet" heard right, but the louder
     sibilants in the songs push note MCD 0.008 dB past its guard, and the speech total doesn't
     move (36.09 → 36.75 %). The female's fricatives are about 10 dB lower re her vowel than
     the male's: about 5 dB from her glottal source (vowels too loud for her fold size), about
     5 dB from the body scale.
   - Experiment 15 fixed the affricates' timing in the renders: a voiceless affricate's /ʃ/
     constriction now held until the folds close, so the 50 ms [h] after a 35–40 ms frication
     became 70–80 ms of /ʃ/ at the /ʃ/'s own level. "Chain" (male) was heard right, "chicken"
     (female) too once the vowel after /tʃ/ got the stop's aspiration allowance, but the total
     didn't move (36.09 → 37.09 %): the male "chicken" lost its short /ɪ/, and the female /tʃ/,
     though well formed, is still about 20 dB below her vowel. The affricates are now limited by
     the same thing as /ʃ/: the female fricative level. The patch is logged in experiment 15, so
     it can be retried with a fix for that level.
   - Experiment 20 found why /ʃ/ is 2–3 dB below /s/: not the teeth source's level or weights
     (its Re² term is about 1 dB higher for /ʃ/, and `w_jet` and `w_dist` are 1 except for one
     case), but its spectrum. The source is white to 5 kHz, and the radiation's +6 dB/octave
     favours /s/'s higher peak. The one weight case is the female /ʃ/ before /ɑ/: `w_dist` 0.78,
     because the jet's distance was measured from the channel's narrowest section rather than its
     exit. Fixing that lifts it 2.4 dB but misses "sheet" (already at full weight), and speech
     reads NO CLEAR CHANGE. Next angle: a teeth-source band that follows the jet's v/D (see its
     "Next"). It also found the judge is chaotic: renders 50–100 dB below the signal apart flip
     Whisper's words, so about ±2 points of any small change's total is noise.

7. **Stressed vowels in running speech (target 2).** The planned durations are not what
   crushes them. Experiment 14 printed `plan_phrases`' segments: the stressed /ʊ/ of "pushes" is
   planned at 121 ms, "eats" /i/ 93, "sweet" /i/ 110, "paints" /eɪ/ 158, "steady" /ɛ/ 123,
   "reads" /i/ 119, in line with Klatt's rules for connected speech. The loss is in how
   the gestures realise the plan:
   - after an aspirated stop, about 50 ms of the vowel is aspiration (the "pu" of "pushes" voices
     from 0.90 s, planned 0.847);
   - before a voiceless fricative, the abduction is complete 15 ms before the fricative's onset,
     so voicing stops 30–35 ms early;
   - before a stop, the closure lands about 17 ms early ("eats", "sweet"), and the vowel's
     energy falls over the 30 ms before it.
   Experiment 14 retimed the fricative abduction later: WORSE, since the fricative onsets get
   weaker. Leave that timing alone. What's left to try: lengthen the stressed vowel in the plan
   itself (the aspiration allowance is 20 ms against a 50 ms VOT), or the stop-closure lead (it
   moves every stop and the sung consonants, so watch the singer's timing guard).
   - Experiment 17 measured both. The closure lead varies by vowel (/t/ lands 7 ms early after
     /i/, 23 ms late after /ɑ/), but aiming it later doesn't save the /i/. The /i/ of "sweet" and
     "eats" undershoots: the /w/ and /ɝ/ before it hold the tongue body to their ends. Length is
     the lever: an aspiration allowance of 50 ms (the VOT), given only where the stop is
     aspirated, fixes "paints" in both voices, but the total stays at 36.09 %. See its "Next".

8. **The pop on "sharp" (target 3).** Closed by experiment 16: the /t/, its tip already raised
   by the /ɹ/, sealed 19 ms early behind the closed lips and the tongue and jaw pumped the
   trapped air to 10.5x the lungs'. Aimed 25 ms later, it seals as the lips open (1.3x,
   −6.8 dB). Other front-to-back clusters after a vowel (/pk/, /bd/, /tk/) still reach 3–6x;
   see experiment 16's "Next".

9. **How a spoken utterance begins, and a click on every /f/ and /v/.** Experiment 18. Speech's
   breath lead (full drive 20 ms before the first segment) is not what loses the first word:
   timed like a sung phrase's it moves the first vowel's voicing 20 ms later and no first-word
   error changes. What it found instead is a geometry click on every /f/ and /v/ (−15 dB at a
   phrase start, −10 dB mid-word, louder than the vowels) from two steps in the labiodental
   geometry (`anatomy/human.rs`). The fix removes it and keeps the held /f/ within 1 dB, but
   Whisper's total doesn't move (36.09 → 37.09 %). The click is closed by experiment 19, which
   kept experiment 18's patch under the artifact keep rule (`an_f_does_not_click` passes, +0.3
   to +4.6 dB). The first-word errors stay open; see experiment 18's "Next".

## Experiments

### 1. A trained singer's jitter, not a clinical one (kept)

- Hypothesis: at the default roughness (0.1), which the comparison sings with, the source
  jittered 0.65 % and shimmered 4.2 % per cycle. Those values are near the clinical thresholds
  (MDVP: 1.04 % and 3.81 %), and the synth's YIN aperiodicity was twice hers (0.010 against
  0.005). A healthy trained voice has about 0.3–0.5 % jitter. Less jitter should lower
  aperiodicity and leave the spectrum alone.
- Change: `apply_source` in `crates/chimera/src/voice/human_voice.rs`: jitter
  `0.003 + 0.035·roughness` → `0.0021 + 0.0245·roughness` (0.45 % at 0.1, 2.7 % at 1).
  Shimmer unchanged. Tried, against the baseline:
  - jitter and shimmer both 0 (diagnostic only): 1.173, aperiodicity cost 1.30; WORSE, note MCD
    +0.86 dB. The averaged log-mel spectra need some perturbation between the harmonics.
  - jitter 0.35 %, shimmer 2.3 %: 1.157; WORSE, note MCD +0.24 dB, and one note fewer for en016a's
    voiceless onsets.
  - jitter 0.35 %, shimmer unchanged: 1.201, MCD +0.06; WORSE on the en016a note alone.
  - jitter 0.45 %, shimmer unchanged: 1.245; BETTER. Kept.
  - The en016a note (no. 100, "tak" at 58.59 s) is a note the synth never voices (0 % voiced in
    the baseline too). Its onset was one stray voiced frame 70 ms before the note, and any change
    to the random sequence can drop it. The guard decided; the gap is a voicing fault (lead 5).
- Singer: training score 1.433 → 1.245 (−0.188 like for like), held-out 1.590 → 1.402.
  Aperiodicity cost 2.88 → 1.74 (synth median 0.010 → 0.008; held-out cost 2.09 → 0.96). Note MCD
  9.243 → 9.288 dB. No other term moved by more than 0.02.
- Speech: word error rate 38.74 → 39.07 % (OK).
- Speed: `os=2` 18.4 % → 18.5 %.
- Tests: pass. Two things for the batch PR:
  - `CHIMERA_BLESS=1 cargo test --workspace` stops at the first failing test binary. `engine.rs`
    reads `beatbox-bar.f32` and fails before `golden_planning` can bless it, so bless with
    `--no-fail-fast`.
  - `golden_analysis` checks the power spectrum of `voice-sustain-male.f32` against TypeScript-era
    numbers in `analysis.json`. Once the voice fixtures are re-blessed it fails on any change to
    the sound. The PR has to update that expectation or analyse a fixed signal instead.
- Commit: 2c41618.
- Next: don't lower shimmer again: the note MCD guard is close. The rest of the aperiodicity cost
  is in songs with voicing gaps (en043a, en018a), which is lead 5, and the largest single cost is
  still level 4–8 kHz (lead 1).

### 2. The folds close over a return phase, not at a corner (kept)

- Hypothesis: at tension 0.55, which the comparison sings with, the glottal area pulse closed
  with sharpness 0.635: close to a hard corner, as if the folds met all at once. Real folds close
  along their length and depth over a return phase, so the source spectrum falls faster above
  about 1 kHz. The synth had 7.5 dB too much at 4–8 kHz and 4.4 dB too much at 2–4 kHz, on every
  vowel. A softer closure should lower both bands and leave the low bands and H1−H2 roughly
  alone.
- Change: `apply_source` in `crates/chimera/src/voice/human_voice.rs`: closure sharpness
  `0.25 + 0.7·tension` → `0.4·tension` (0.22 at tension 0.55; speech, at 0.5, gets 0.2). Tried,
  against experiment 1:
  - `0.1 + 0.55·tension` (0.40): 1.047; BETTER. 4–8 kHz cost 2.91, 2–4 kHz 0.60.
  - `0.4·tension` (0.22): 0.930; BETTER. Kept.
  - `0.2·tension` (0.11): 0.915; BETTER, but the synth's 4–8 kHz median (−25.6 dB) passes hers
    (−24.8), MCD is flat (8.628) and it leaves tension almost no say in the closure. The extra
    0.015 is not worth that.
- Singer: training score 1.245 → 0.930 (−0.314 like for like), held-out 1.402 → 1.045.
  - Level 4–8 kHz cost 5.37 → 0.68 (synth median −17.3 → −23.7 dB, hers −24.8).
  - Level 2–4 kHz 1.43 → 0.13 (−14.0 → −17.1, hers −18.4).
  - Aperiodicity 1.74 → 0.97 (held out 0.96 → 0.05): YIN reads less high-frequency energy
    between the harmonics.
  - H1−H2 2.09 → 2.21: within the 0.25 a term may rise.
  - Level 0–0.5 kHz 1.41 → 1.32. Note MCD 9.288 → 8.624 dB. Nothing else moved by more than
    0.07.
- Speech: word error rate 39.07 → 39.07 % (OK).
- Speed: `os=2` 18.5 % → 19.1 % (3 %).
- Tests: pass, including `golden_analysis`.
- Commit: 8c04752.
- Next: the high bands are close to her now. The largest costs are H1−H2 (2.21), the onsets
  (1.39 and 1.10), level 0–0.5 kHz (1.32) and aperiodicity (0.97). H1−H2 and the low band point
  at the open quotient and how it follows pitch (lead 1), not at the closure.

### 3. A posterior glottal chink that stays open through closure (kept)

- Hypothesis: H1−H2 (cost 2.21) and level 0–0.5 kHz (1.32) were too low. At the default
  breathiness the folds sealed almost completely (chink 0.007 cm² on the female body). Most voices,
  women's and trained singers' included, keep a small posterior gap through the closed phase
  (Södersten & Lindestad 1990). Its leak lifts H1 and damps the tract, so H1−H2 and the low band
  should rise and H1−H2 should swing less as the harmonics cross F1.
- Change: `apply_source` in `crates/chimera/src/voice/human_voice.rs`: chink base 0.004 → 0.008
  (`0.008 + 0.03·breathiness`, times k²). Tried, against experiment 2:
  - 0.02: 0.896; WORSE, seven notes fewer measured (H1−H2 and pitch on en008a, en009a, en018a and
    more). Every lost note is one the synth hardly voices (5–10 % voiced: short notes on a stop
    coda like "dɑk", "ɹaɪt", "ɪt"); one stray voiced frame decides whether they are measured.
  - 0.016: 0.883; WORSE, the same kind of notes.
  - 0.012: 0.878; WORSE: one note fewer for H1−H2 (en048a "lʊk", 10 % voiced) and one fewer long
    enough for vibrato (en033a no. 96, a 0.6 s /ə/ whose voiced run fell under 0.4 s).
  - 0.008: 0.889; BETTER. Kept.
  - The larger chinks scored better but lose more of these notes. That is lead 5 again: notes
    the synth barely voices.
- Singer: training score 0.930 → 0.889 (−0.041 like for like), held-out 1.045 → 1.018.
  - Level 0–0.5 kHz 1.32 → 1.14 (synth median −7.31 → −7.04 dB, hers −4.02).
  - H1−H2 2.21 → 2.14 (−1.21 → −0.96 dB, hers +3.20). The swing with pitch is unchanged: by
    pitch the synth's medians still run −5 dB at MIDI 54 to +12 at 72, hers +2 to 0.
  - Level 4–8 kHz 0.68 → 0.44, level 2–4 kHz 0.13 → 0.00, aperiodicity 0.97 → 0.90.
  - Note MCD 8.624 → 8.573 dB. Nothing else moved by more than 0.02.
- Speech: word error rate 39.07 → 39.40 % (OK).
- Speed: `os=2` 19.1 % → 18.2 %.
- Tests: pass, including `golden_analysis`.
- Commit: f5cc671.
- Next: a larger chink doesn't flatten H1−H2's swing with pitch, and it costs notes the synth
  barely voices. Try a wider F1 bandwidth or F1 that follows pitch (lead 1). Lead 5 is worth
  a run of its own: those barely voiced short notes before a stop make every guard noisy.

### 4. The folds stay apart from the in-breath until the first voiced segment (reverted)

- Hypothesis: a phrase starts with the folds adducted (abduction 0) while the breath gesture
  ramps the pressure up from 60 ms before the first segment. So the voice starts as soon as the
  pressure crosses threshold: 15–20 ms before a vowel-initial note (hers +5 ms), and as a voiced
  blip before a voiceless onset (/s/, /f/, /h/ voiced 100–130 ms early). A real breath group
  starts from the open glottis of the in-breath, and the folds close only into the first voiced
  sound. Holding them open until then should move both onset terms later and leave the rest
  alone.
- Change: `plan_utterance` in `crates/chimera/src/phonetics/planner.rs`: an "inhale" abduction
  gesture (0.3, strength 6, like the voiceless one) from 50 ms before the plan start to 30 ms
  before the first voiced segment, ramping out over 30 ms. Tried, against experiment 3:
  - on every phrase: 1.378; WORSE. Vowel-initial onset −0.015 → −0.040 s, voiceless-initial
    −0.024 → −0.095 s, voiced share 0.260 → 0.634 cost, aperiodicity 0.90 → 2.50, 28 song
    measures on fewer notes. In the render (en001a at 2.40 s) the vowel now voices on time, but
    a 54 dB [h] (the vowel is 78 dB) runs for 55 ms before it: full breath through an open
    glottis. YIN counts that as the voice starting.
  - only on phrases that open on a voiceless consonant: 1.134; WORSE. Vowel-initial unchanged,
    voiceless-initial still −0.095 s: the breath blows through the open glottis before the
    onset's oral closure is made, an [h] before /k/, /t/, /p/. The gesture also reached back into
    the previous phrase when the gap was short (en002a "dh_eo" at 9.41 s, 135 ms before
    "p_l_ei_n_z": voiced 1.0 → 0.0), which cost voiced share and aperiodicity.
- Singer: training score 0.889 → 1.378 / 1.134 (not kept); held-out 1.018 → 1.314 (first try).
- Speech: not measured (the singer verdict was WORSE).
- Verdict: WORSE on both tries: onset terms rose instead of falling, voiced share and
  aperiodicity rose, songs measured on fewer notes.
- Next: don't open the glottis at the phrase start while the breath leads the articulation.
  The breath's 60 ms lead over the first segment is the real fault. Time the breath ramp to the
  first segment's oral closure (or, for a vowel, end it at the vowel's start), so that pressure
  builds behind a closure or into adducted folds, not into the open air (lead 3).

### 5. A trained singer's vibrato rate, 6 Hz rather than 5.5 (reverted)

- Hypothesis: the synth puts every vibrato at 5.5 Hz (measured 5.49), and hers runs at 6.1
  (6.0 over all her notes). Trained singers' vibrato is 5.5–6.5 Hz, about 6 Hz on average
  (Prame 1994, Sundberg). Raising the source's default rate should close the vibrato-rate gap
  (cost 0.66) and leave everything else alone, since only the pitch modulation's speed changes.
- Change: the default `vibrato_rate` 5.5 → 6.0 Hz in `DEFAULT_SOURCE`
  (`crates/chimera/src/voice/human_voice.rs`) and in the glottis default
  (`crates/chimera/src/sources/glottis.rs`). Speech has no vibrato, so it is untouched. Tried,
  against experiment 4's best (experiment 3, 0.889):
  - 6.0 Hz: 0.845 (−0.044 like for like); WORSE, en009a and en048a lose one H1−H2 and pitch note
    each.
  - 5.9 Hz: 0.843 (−0.046); WORSE, en019a, en033a (one note fewer long enough for vibrato), en034a,
    en048a and one more each lose a note.
  - 6.1 Hz: 0.846 (−0.044); WORSE, en019a loses one H1−H2 and pitch note.
- Singer: training 0.889 → 0.845 at 6.0 Hz (not kept), held out 1.018 → 1.007–1.008 at 5.9 and
  6.1 Hz. Vibrato rate cost 0.66 → 0.00 at every rate (synth 5.49 → 5.87 / 5.98 / 6.08 Hz, hers
  6.08); vibrato share 0.59 → 0.60–0.64; nothing else moved by more than 0.03.
- Speech: not measured (the singer verdict was WORSE).
- Verdict: WORSE on the fewer-notes guard at every rate. Every note that flips is a short note
  (0.08–0.34 s, mostly a stop coda: "aʊt", "lʊk", "ɹis", "mʌst") that the synth voices 0–12 % of
  its length, so one stray voiced frame decides whether it is measured. Notes were also gained
  ("dɑk" en015a, "ɪt" en045a, "noʊt" en011a): the flips go both ways, as noise. The vibrato
  oscillator runs through these notes even where the gesture gives them no depth, so its phase
  is enough to move a frame. I stopped at three rates rather than search for a rate that happens
  to pass the guard.
- Next: fix lead 5 first. These barely voiced short notes have now blocked or limited three
  experiments (1, 3, 5). Once they voice reliably, retry 6 Hz: it is worth about −0.04.


### 6. A phrase holds its last note to the end; the breath comes from the rest (reverted)

- Hypothesis: lead 5's barely voiced notes are mostly phrase-final. Of the 368 notes the synth
  voices under 30 % of their steady part while she voices them over 70 %, 315 end a phrase.
  `plan_song` ends a phrase's last vowel and its coda `min(0.12, 0.3·note)` before the note's end
  "to leave a breath before the next phrase". But both callers (`compare csd` and `render notes`)
  split phrases at rests and call `plan_song` once per phrase, so the breath has the rest after
  the note. A 0.12 s "aʊt" gave its vowel and /t/ 0.08 s, and the steady window (30–80 % of the
  note) caught the /t/. A singer holds the note to its notated end and puts the coda on the
  release. Holding the last phrase of a `plan_song` call to the note's end should raise voiced
  share and lower aperiodicity, and leave a mid-call phrase end (no rest) as it was.
- Change: `plan_song` in `crates/chimera/src/phonetics/sing.rs`: the reserve stays
  `min(0.12, 0.3·note)` for a phrase that runs straight into another in the same call, and
  becomes `min(r, 0.3·note)` for the call's last phrase. Tried, against experiment 3:
  - r = 0: 0.939 (+0.050); WORSE. Voiced share cost 0.26 → 0.00, aperiodicity 0.90 → 0.00, but the
    voice decays for about 50 ms after the plan ends (the breath's 50 ms ramp-out; the folds part
    45 ms after a sonorant), so it runs past the note into the next phrase's onset search, which
    starts 30 ms after the previous note. Vowel-initial onset −0.015 → −0.040 s (cost 1.39 → 3.44),
    voiceless-initial cost 1.11 → 1.40, level 0–0.5 kHz 1.14 → 1.44.
  - r = 0.05 (the breath's release, so the sound stops at the note's end): 0.747 (−0.142 like for
    like); WORSE, en036a and en044a lose one voiceless-initial onset each.
  - r = 0.04: 0.733 (−0.156); WORSE, the same two onsets and one H1−H2 and pitch note in en048a.
  - r = 0.06: 0.762 (−0.127); WORSE, the en044a onset.
- Singer at r = 0.05: training 0.889 → 0.747, held-out 1.018 → 0.98 (from the held-out costs).
  Voiced share cost 0.26 → 0.04 (synth 0.876 → 0.896, hers 0.927), aperiodicity 0.90 → 0.03,
  H1−H2 2.14 → 2.29, level 0–0.5 kHz 1.14 → 1.28, onsets 1.39 → 1.47 and 1.11 → 1.14. Note MCD
  8.573 → 8.537 dB, envelope correlation 0.797 → 0.810. Nothing else moved by more than 0.05.
- Speech: not measured (the singer verdict was WORSE). Speech doesn't use `plan_song`.
- Verdict: WORSE on the fewer-notes guard at every value. The lost onsets are two phrase-initial
  notes the synth never voices, before or after (en044a no. 145 "pæ", en036a no. 71 "ki"). Their
  onset was a stray voiced frame 80–100 ms before the note, which the change removed. The
  rules' answer is revert; the gain is real but waits on those notes.
- Next: fix the vowel that never phonates after a phrase-initial voiceless stop (lead 5's second
  note: aspiration, then 50 ms at −55 dB where the vowel should be). Then retry this change at
  r = 0.05: it is worth about −0.14, the largest single gain so far. H1−H2 and the low band rose
  a little with it, as more of the short notes' vowels are now measured.

### Judge change after experiment 6: like for like, note by note

Experiments 1, 3, 5 and 6 were blocked or held back by the per-song coverage guard. It flipped
on notes the synth barely voices (0–12 %), where one stray voiced frame decides whether a note
has an onset, H1−H2 or pitch reading. Per-note voicing has no gap to put a threshold in.

The verdict now recomputes each song's value of each term for both runs, over the notes on
which both runs give it. `score.json` keeps every note for this. A note that stops or starts
being measured leaves both runs alike. Not voicing a note still costs, through the voiced share.
The coverage counts are gone, and the score itself is unchanged: 0.889 at the current best,
and a rerun is NO CLEAR CHANGE.

Checked on experiment 5's change (vibrato 5.5 → 6.0 Hz): BETTER, −0.043 like for like. It
was not kept: that is for an experiment to do. Experiments 5 and 6 are worth retrying as they
were.

### 7. Experiment 6 again under the like-for-like judge: the last note held to the end (reverted)

- Hypothesis: as in experiment 6. `plan_song` ends a phrase's last vowel and coda
  `min(0.12, 0.3·note)` before the note's end to leave a breath, but the call's last phrase is
  followed by a rest (both callers split phrases at rests), so the breath comes from the rest. The
  steady window of short phrase-final notes caught the coda instead of the vowel. Holding the
  call's last note to 50 ms before its end (the breath's release) should raise voiced share and
  lower aperiodicity. Experiment 6 measured this at −0.142 and failed only the coverage guard,
  which the judge change after it removed.
- Change: `plan_song` in `crates/chimera/src/phonetics/sing.rs`: the reserve is
  `min(0.12, 0.3·note)` for a phrase that runs straight into another in the same call, and
  `min(0.05, 0.3·note)` for the call's last phrase. One value, the one experiment 6 settled on.
- Singer: training score 0.889 → 0.747 (−0.145 like for like, BETTER), held-out 1.018 → 0.983.
  - Voiced share cost 0.26 → 0.04 (synth 0.876 → 0.896, hers 0.927).
  - Aperiodicity 0.90 → 0.03.
  - H1−H2 2.14 → 2.29 and level 0–0.5 kHz 1.14 → 1.28: more of the short notes' vowels are now
    measured, and they have the pitch-dependent H1−H2 of lead 1.
  - Vowel-initial onset 1.39 → 1.47; voiceless-initial 1.11 → 1.14.
  - Note MCD 8.573 → 8.537 dB, envelope correlation 0.797 → 0.810. Nothing else moved by more than
    0.05.
- Speech: word error rate 39.4 → 39.4 % (OK; speech doesn't use `plan_song`).
- Speed: `os=2` 18.2 % → 18.4 %.
- Tests: `bun test tests` passes (18). After blessing twice, `cargo test --workspace` fails one
  test: `songs_match_typescript` (`golden_planning.rs`), "Made of air (male)" gestures[138].t1
  11.6066 vs 11.6766. That is the last note's vowel ending 70 ms later (0.12 → 0.05 s), the change
  itself. The baseline passes it. The test compares `plan_song` against TypeScript-era plans in
  `tests/golden/planning.json`, and `CHIMERA_BLESS` doesn't regenerate them (the TypeScript engine
  is gone), so it isn't a blessed fixture. The rules' only exception is the airway checks in
  `golden_analysis`.
- Verdict: reverted on a failing test, as the rules require, though every judge passed. Editing or
  re-baselining the test to let the change through is not this experiment's to do.
- Next: the owner should decide whether the song plans in `planning.json` may be re-blessed after
  an intended planner change (as the sound fixtures are), or add a rules exception for
  `songs_match_typescript` like the airway one. Then keep this change as it is: it is worth −0.145
  and takes voiced share and aperiodicity to about 0. Until then no `plan_song` change can be kept,
  so the next run should pick a lead outside `plan_song`: the vibrato rate (lead 4, about −0.04,
  in the source), or F1 bandwidth for H1−H2 (lead 1).

### Decision after experiment 7: the planners' snapshots are blessed too

`golden_planning` compared the planners against TypeScript-era plans in `planning.json`. Blessing
didn't regenerate those, so no change to `plan_song` (or any planner) could pass the tests.
Like the audio fixtures, these are snapshots of a retired engine. `CHIMERA_BLESS=1` now
rewrites the utterance, duration, phrase, song and live-note plans in `planning.json`, keeping
its inputs. Blessing an unchanged planner rewrites the same values (worst difference 2e-16).
The beat sequencer's expectations stay fixed.

Experiment 7's change (a phrase's last note held to 0.05 s before its end, −0.145 like for
like, speech and bench OK) can now be retried as it was.

### 8. Experiment 7 again, now that the planners' snapshots bless: the last note held to the end (kept)

- Hypothesis: as in experiments 6 and 7. `plan_song` ends a phrase's last vowel and coda
  `min(0.12, 0.3·note)` before the note's end to leave a breath, but the call's last phrase is
  followed by a rest (both callers split phrases at rests), so the breath comes from the rest. On
  short phrase-final notes the steady window caught the coda, not the vowel. Holding the call's
  last note to 50 ms before its end (the breath's release) should raise voiced share and lower
  aperiodicity. Experiment 7 was BETTER on every judge and reverted only on
  `songs_match_typescript`, which the decision after it turned into a blessed snapshot.
- Change: `plan_song` in `crates/chimera/src/phonetics/sing.rs`: the reserve is
  `min(0.05, 0.3·note)` for the call's last phrase and stays `min(0.12, 0.3·note)` for a phrase
  that runs straight into another in the same call. One value, experiment 7's.
- Singer: training score 0.889 → 0.747 (−0.145 like for like, BETTER), held-out 1.018 → 0.983.
  The same numbers as experiment 7:
  - Voiced share cost 0.26 → 0.04 (synth 0.876 → 0.896, hers 0.927); aperiodicity 0.90 → 0.03.
  - H1−H2 2.14 → 2.29 and level 0–0.5 kHz 1.14 → 1.28: more short notes' vowels are measured, with
    lead 1's pitch-dependent H1−H2.
  - Vowel-initial onset 1.39 → 1.47; voiceless-initial 1.11 → 1.14.
  - Note MCD 8.573 → 8.537 dB, envelope correlation 0.797 → 0.810. Nothing else moved by more than
    0.05.
- Speech: word error rate 39.4 → 39.4 % (OK; speech doesn't use `plan_song`).
- Speed: `os=2` 18.2 % → 18.4 %.
- Tests: after blessing twice (the first run fails `engine.rs` on the beatbox fixture, as
  expected), `cargo test --workspace` passes, `golden_planning` and `golden_analysis` included, and
  `bun test tests` passes (18).
- Commit: 31643bb.
- Next: the largest costs are now H1−H2 (2.29), the onsets (1.47 and 1.14) and level 0–0.5 kHz
  (1.28). The cheapest next gain is experiment 5's vibrato rate (5.5 → 6.0 Hz, about −0.04, lead
  4), which the like-for-like judge already called BETTER. For H1−H2, try a wider F1 bandwidth or
  F1 that follows pitch (lead 1).

### 9. Experiment 5 again under the like-for-like judge: a trained singer's vibrato rate, 6 Hz (kept)

- Hypothesis: as in experiment 5. Every vibrato ran at 5.5 Hz (measured 5.50), and hers runs at
  6.1. Trained singers' vibrato is 5.5–6.5 Hz, about 6 Hz on average (Prame 1994, Sundberg).
  Raising the source's default rate should close the vibrato-rate gap (cost 0.64) and leave the
  rest alone. Experiment 5 measured −0.044 and failed only the coverage guard, which the judge
  change after experiment 6 replaced; that judge already called it BETTER.
- Change: the default `vibrato_rate` 5.5 → 6.0 Hz in `DEFAULT_SOURCE`
  (`crates/chimera/src/voice/human_voice.rs`) and in the glottis default
  (`crates/chimera/src/sources/glottis.rs`). One value, experiment 5's first.
- Singer: training score 0.747 → 0.714 (−0.034 like for like, BETTER), held-out 0.983 → 0.978.
  - Vibrato rate cost 0.64 → 0.00 (synth 5.50 → 5.98 Hz, hers 6.08); held-out 0.09 → 0.00.
  - Vibrato share 0.59 → 0.65 and depth 0.04 → 0.02: the detector reads a few notes differently
    at the new rate.
  - Aperiodicity 0.03 → 0.06, voiceless-initial onset 1.14 → 1.08, vowel-initial 1.47 → 1.49,
    H1−H2 2.29 → 2.27. Note MCD 8.537 → 8.535 dB, envelope correlation 0.810 → 0.810, pitch error
    4.81 → 4.86 cents. Nothing else moved by more than 0.01.
- Speech: word error rate 39.4 → 39.4 % (OK; speech has no vibrato).
- Speed: `os=2` 18.4 % → 19.4 %, then 18.4 % on the rerun.
- Tests: after blessing twice (the first run fails `engine.rs`, as expected), `cargo test
  --workspace` passes, `golden_analysis` and `golden_planning` included, and `bun test tests`
  passes (18). The blessed `planning.json` changed too (the live-note plans carry the rate).
- Commit: ec79db0.
- Next: the vibrato group's last cost is the share (0.65): the synth vibratos every long note,
  she about two thirds. The largest costs are still H1−H2 (2.27), the onsets (1.49 and 1.08) and
  level 0–0.5 kHz (1.28). For H1−H2, try a wider F1 bandwidth or F1 that follows pitch (lead 1);
  for the onsets, time the breath ramp to the first segment (lead 3).

### 10. A sung phrase's breath reaches full drive at its first segment (kept)

- Hypothesis: every phrase's breath gesture ramped up over the 40 ms ending 20 ms before its first
  segment, so the lung pressure crossed the phonation threshold early and a sung phrase voiced
  ahead of its note: 16 ms early before a vowel (hers 5 ms late) and 20 ms early before a voiceless
  onset (hers 18 ms late). A sung onset is planned (`plan_song` puts the first vowel on the beat),
  so a singer times the breath to it: the pressure builds into the adducted folds, or behind the
  first constriction, as the first segment starts. The two onset terms should move later and
  nothing else should move much.
- Change: `plan_utterance` in `crates/chimera/src/phonetics/planner.rs`: for a sung phrase
  (`sung`, set only by `plan_song`), the breath gesture reaches full drive at the first segment
  (`BREATH_SUNG_AT` = 0.06 s after the plan start, which is where the first segment starts)
  instead of 0.04 s. Speech and live notes keep the old lead, so speech is untouched and live
  notes get no added latency. Tried, against experiment 9:
  - 0.05: 0.661 (−0.048 like for like); BETTER. Onsets −0.005 / +0.020 s, costs 1.18 / 0.77.
  - 0.06: 0.623 (−0.085); BETTER. Kept: the breath peaks just as the first segment starts.
  - 0.07: 0.647 (−0.063); BETTER. Onsets +0.015 / +0.006 s, costs 1.10 / 0.68.
- Singer: training score 0.714 → 0.623 (−0.085 like for like, BETTER), held-out 0.978 → 0.791.
  - Onset, vowel-initial cost 1.49 → 0.97 (synth median −0.016 → +0.004 s, hers +0.005); held-out
    1.16 → 0.00.
  - Onset, voiceless-initial 1.08 → 0.52 (−0.020 → +0.037 s, hers +0.018); held-out 1.14 → 0.05.
  - Envelope correlation 0.810 → 0.814, note MCD 8.535 → 8.535 dB, pitch error 4.86 → 4.85 cents.
    No other term moved by more than 0.01.
- Speech: word error rate 39.4 → 39.4 % (OK; speech isn't `sung`).
- Speed: the bench doesn't run the planner, so its render path is unchanged. The machine was
  loaded (load average 30 on 16 cores, from other sessions): `os=2` read 34.0 % and 33.6 %, and the
  previous commit read 33.1 % against this one's 33.2 % in back-to-back runs. `out/hill/best/bench.txt`
  keeps the last quiet-machine figure (19.4 %).
- Tests: after blessing twice (the first run fails `engine.rs`, as expected), `cargo test
  --workspace` passes, `golden_planning` and `golden_analysis` included, and `bun test tests`
  passes (18).
- Commit: 3747f99.
- Next: the largest cost by far is H1−H2 (2.27, a sixth of the score on its own), then level
  0–0.5 kHz (1.28), the onsets' remaining mean gap (0.97 and 0.52) and the vibrato share (0.65).
  For H1−H2, try a wider F1 bandwidth or F1 that follows pitch (lead 1). For the onsets, look at the
  phrases still far off rather than the medians, which are now about right.

### Batch review (Codex), and what changed after it

The review found no engine defect and called all six changes plausible. Three things changed:

- **Speech and speed are judged against the batch's baseline** (`out/hill/baseline/`), not
  the best run. With a 3-point or 10 % allowance per step measured against the best run,
  small losses could add up over a batch: 38 → 41 → 44 → 47 % would pass every step.
- **Fixtures are written atomically** (a temporary file, then a rename). While one planning
  test blessed `planning.json`, another could start and read half of it.
- **The pressed end of the tension control is back.** Experiment 2's `0.4·tension` capped the
  closure at 0.4 even at full "Pressed", where it had been 0.95. That is a sound no judge
  measures, since everything is rendered at tension 0.5–0.55. Above 0.6 the closure now
  sharpens again, to 0.95 at full tension. Every judged render is byte-identical, so no score
  moves. A test (`tests/voice_source.rs`) keeps the closure rising with tension all the way.


### Listening result (2026-11-07): the pilot's answer is yes

Blind A/B, singer first, every clip at the same loudness (5 song excerpts, 4 sentences):
**now 7, before 0, can't tell 2** (en006a and harvard-male-07). Every held-out excerpt went to
"now". So the score's direction matches the user's ears.

What they heard, in their words, and what the renders show:

- **"Mostly a brightness change; the darker one sounds better as a default."** "Now" is the darker
  one. Its spectral centroid is about 150 Hz lower in every song clip (White Christmas
  1110 → 943 Hz; hers is 807), and 2–8 kHz is 3–8 dB down. The user wants a control for it.
- **The gains feel incremental.** The user is more interested in pronunciation, which the singer
  score doesn't measure.
- **"Sheet" heard as "feet".** In a fresh render of "sheet" the /ʃ/ is 23 dB below its vowel, the
  same as /f/ in "feet". Whisper hears "feet" in both voices too, and "fire" for "sharp"
  (female). Every word with ch or j fails in Whisper as well: juice → "roost", chair →
  "coloring", chain → "pin", birch → "bird", chopped → "hopped", chicken dropped.
- **J sounds very muted.** The /dʒ/ of "juice" is about 25 dB below its vowel, and mostly
  low-frequency voicing.
- **A huge pop on the p of "sharp".** The /p/ release peaks 7 dB above the loudest point of the
  vowel, and its energy is all below 4 kHz: a pressure thump, not a burst.
- **A tinny noise at syllable ends (after "truck").** It is a 750 Hz buzz, with lines at exact
  multiples of 750 Hz about 25 Hz wide. 750 Hz is the control rate: 48 kHz ÷ 64, one update
  every 64 samples. The /k/ release of "truck" is mostly this buzz, and it is in the decay after
  every word checked (sheet, feet, juice, pushes). Steady fricatives are clean noise.
- **"Pu" of "pushes" inaudible.** The stressed /ʊ/ lasts about 40 ms; the unstressed "-es"
  gets about 100 ms. The text front end marks the stress correctly (pˈʊʃəz).

Whisper agrees with the user on the word errors (sheet → feet, and the j and ch words), which
supports using it as the pronunciation judge. It disagrees on one: it heard "pushes" correctly in
the female voice after the batch (before it heard "puffs"). It can't judge the pop or the buzz,
since neither changes the words, so those need checks of their own.

## Batch 2: pronunciation (from 2026-11-07)

Batch 1 is merged (PR #16). The owner's listening (above) asked for pronunciation, so batch 2's
judge is Whisper. See the README's "current batch" section for the rules.

### Before the batch: the fixes and the Bright control (PR #19, not a hill-climb step)

- **The tinny ring: fixed.** It was a 750 Hz buzz from the lung pressure stepping once per
  control block. It now ramps across the block. Block-periodic share of a word's tail ~20 % →
  under 1 %.
- **The pop on "sharp": not fixed.** The tongue squeezes the air between the closed lips and its
  /t/ seal to 11x the lungs' pressure (88 cmH2O). The /p/ releases at +7 dB re the sentence's
  loudest vowel (male). Absorbing a sealed pocket's wall motion halved it but broke the engine's
  adiabatic-compression test and the creatures' parity; reverted. The ignored test
  `the_p_of_sharp_does_not_pop` in `tests/consonant_artifacts.rs` measures it.
- **Bright control** (`VoiceSource.brightness`, default 0.25 = the current sound).
- Against batch 1's best: singer 0.623 → 0.582 (BETTER, onsets), held-out 0.791 → 0.789; speech
  39.4 → 40.7 %; speed 18.2 % of one core.

### The judge for batch 2

- `tools/asr/sentences.tsv` gained 20 held-out sentences (set column "held"): Harvard list 2, and
  ten unpredictable sentences dense in sh, ch and j. They never decide.
- `speech_wer.py --against` now says BETTER (training word error rate down by at least 1.5
  points), WORSE (up by more than 3) or NO CLEAR CHANGE, and reports the held-out rate.
- The old batch-1 directories are kept as `out/hill/batch1-baseline` and `out/hill/batch1-best`.

### Batch 2 baseline (commit 4980e3b, `out/hill/baseline`)

- Speech, training: **40.73 %** (harvard female 37.5, male 37.5; nonsense female 42.25, male
  46.48). Held-out: **50.67 %** (harvard female 35.44, male 39.24; nonsense female 70.42, male
  60.56).
- Singer: training 0.582, held-out 0.789.
- Speed: 18.2 % of one core (`os=2`).
- The /p/ of "sharp": +7.0 dB re the loudest vowel, oral pressure 11.1x the lungs' (male).

### 11. The space in front of the raised tongue tip is narrow: /ʃ/'s front cavity (reverted)

- Hypothesis: the /ʃ/ of "sheet" peaked at 2.06 kHz (female) and 1.73 kHz (male), against
  2.5–3.5 kHz in real speech, and Whisper heard "feet". A probe of the noise sources showed the
  teeth (obstacle) source makes the /ʃ/: with it off the fricative falls 10 dB, while the
  aspiration source changes nothing. The peak is the resonance of the front cavity between the
  postalveolar constriction and the lips, which it excites. In the area function that cavity
  holds a chamber of 4.7–5.5 cm² over about 1 cm (male, ~10 cm³ in all). It is the space under the
  alveolar ridge and below the raised tip, down to the floor of the mouth (midsagittal depth
  ≈ 1.7 cm). Those rays took the oral law, 2.6·d^1.4, which describes the mid-oral cavity
  between the molars. In front of the tip the airway lies inside the anterior dental arch and the
  sublingual space, far narrower. Lips and jaw were checked first and are not the cause:
  - lip width 0.2 → 0.6: peak +100–200 Hz
  - lip protrusion 0.75 → 0.4: +240 Hz
  - jaw 0.35 → 0.1: nothing
  - tongue tip forward (ttX 5.7 → 6.05): about +100–200 Hz
- Change: `crates/chimera/src/anatomy/human.rs`: a fourth area law, `AreaLaw.ahead`, for the oral
  rays whose midpoint lies in front of the tongue tip, faded in by
  `smoothstep(0, 0.4·k, mid.x − tip.x)`. Vowel and consonant tables regenerated with `chimera
  calibrate`: the vowels' formants moved up to 4–5 % (the open vowels' F1 down, /ɝ/ F2 up).
  After recalibration the mean F1 error is 1.33 → 1.59 % (male) and 2.01 → 2.53 % (female); F2
  and F3 are about the same. Values tried (/ʃ/ of "sheet", peak female / male):
  - α = 2.0 (the dental arch's front law), no recalibration: 2.16 / 1.78 kHz. Too little.
  - α = 1.2, with an uncalibrated sensitivity check of α = 0.7 (3.0 / 2.5 kHz):
    - "Sheet": 2.81 / 2.06 kHz, 1–2 kHz band −5 → −18 dB (female).
    - Speech NO CLEAR CHANGE: 40.73 → 39.40 %. Harvard male 37.5 → 27.5, Harvard female
      37.5 → 46.25.
    - Singer WORSE: 0.582 → 0.590, note MCD 8.536 → 8.648 dB, over its 0.1 dB guard.
  - α = 1.6: "sheet" 2.34 / 1.92 kHz. The numbers below are for this value.
  - /s/ and /f/ barely move at any value: "seat" 4.0–4.1 kHz, "feet" 4.3–4.5 kHz (female).
- Speech: word error rate 40.73 → 35.76 % (training, **BETTER**), 50.67 → 45.33 % (held-out).
  - Moved right: "chicken leg" (male, 5 → 1 errors), "juice" (male), "sharp" and "the thin road"
    (female), "soft hill" (female), "some bright doors" (male).
  - "Sheet" is now heard in the male voice ("screw the sheet": the error moved to "glue"). The
    female still hears "feet": her /ʃ/ is still 21–22 dB below the vowel, as quiet as her /f/.
  - Chair, chain, birch and chopped still fail.
- Singer: training 0.582 → 0.584 (NO CLEAR CHANGE, every guard holds; note MCD 8.536 → 8.628
  dB), held-out 0.789 → 0.810. Level 0–0.5 kHz cost 1.22.
- Speed: `os=2`, back to back with the baseline commit: 18.2 / 19.1 % (baseline) against
  18.4 / 19.7 % (change), so 1–3 %. A first run read 22.2 %, while other sessions loaded the
  machine (load average 13).
- Tests (fixtures blessed twice, then `cargo test --workspace`):
  - `golden_analysis` fails at its first "human |H|" check: the README's airway exception. Its
    later checks (tube networks, FFT, LPC, YIN, Nelder–Mead) don't touch the human tract.
  - `golden_geometry` (the TypeScript-era area function) fails: it has to move with any
    deliberate geometry change. It is the same kind of check as the airway one; the batch PR
    would regenerate it.
  - `golden_planning`'s beat sequencer checks fail by 1e-3 in tongue targets, which come from the
    regenerated vowel table.
  - **`geometry_continuity::no_area_jumps_anywhere_in_the_articulators_speech_ranges` fails:**
    "child ə velum=1.117: 0.32 cm²". The new law doesn't cause it:
    - new law, old tables: passes.
    - old law (α = 2.6), new tables: fails the same way.
    - The recalibrated child /ə/ posture lands on a jump that was already in the geometry. It is
      at the top of the pharynx (the velopharyngeal rays), with the velum raised to about 1.117
      (posture jaw 0.80, tbX 2.70, tbY 7.38, root −0.50).
  - `bun test` was not run.
- Verdict: reverted. The speech is BETTER, the singer guards hold and the speed is fine, but a
  non-fixture test fails, so the rules revert it. This is the same position as experiment 7.
- Commit: none (log only). The patch: in `AreaLaw` add `pub ahead: (f64, f64)`, default
  `(1.6, 1.4)`. In the region-2 branch of the area loop, replace `a_o·d^b_o` with
  `lerp(a_o·d^b_o, a_h·d^b_h, smoothstep(0, 0.4·k, g.mid.x − tip.x))`. Then run `chimera calibrate
  vowels all` and `chimera calibrate consonants`.
- Next:
  - Fix the velopharyngeal jump that `geometry_continuity` finds for the recalibrated child /ə/
    (velum ≈ 1.117) as its own step, then retry this change as it was.
  - Retry α = 1.2 too: it moved /ʃ/ further (2.8 kHz female), and its speech result (−1.33)
    was within Whisper's run-to-run noise of α = 1.6's.
  - After that, the /ʃ/'s level: the female /ʃ/ is 21 dB below its vowel and the male 11 dB.
    The teeth source's gain, or how its weight depends on the distance behind the teeth (2.5 cm
    for /ʃ/, just short of where `w_dist` starts to fade, at 2.6), is the place to look.

### Decision after experiment 11: the geometry and beat-sequencer snapshots are blessable

Experiment 11 changed the mouth's area rule in front of the tongue tip and recalibrated the vowel
tables, as the rules ask. Two TypeScript-era snapshots then failed by design: `golden_geometry`
(the area functions) and the beat sounds' and beat player's gestures in `planning.json` (their
vowel targets moved by 0.001). Like the audio and planner fixtures in batch 1, they are now
blessed with `CHIMERA_BLESS=1` (`golden_geometry.rs`, `golden_planning.rs`). On the current engine
blessing them changes no value (geometry within 7e-14; planning only in number formatting and
two last-digit floats). The beat notation stays fixed.

`geometry_continuity` is a real check, not a snapshot, so experiment 11's failure there stands: its
recalibrated child /ə/ posture lands on a jump in the area function at the top of the pharynx,
near the raised velum. That jump was already there; experiment 12 fixes it together with
experiment 11's change.

### 12. Experiment 11 again, with the velopharyngeal area jump fixed (kept)

- Hypothesis: as in experiment 11. The /ʃ/'s front cavity, in front of the raised tongue tip,
  took the mid-oral area law, so its resonance sat at 1.7–2.1 kHz and Whisper heard "feet" for
  "sheet". Experiment 11's narrower law (`AreaLaw.ahead`) fixed that. It was reverted only
  because the recalibrated child /ə/ landed on an area jump that was already in the geometry.
  That jump is a fault in how the area function is computed, so fixing it belongs to this change.
- The jump's cause: a probe of the child /ə/ posture (jaw 0.80, tbX 2.70, tbY 7.38, root −0.50)
  found it at velum 1.11470 → 1.11471, on the second oral-fan ray (175°, still region 1, behind
  the velum). The ray's raw gap is continuous (0.602 cm). Its gap after the soft-tissue roof
  contact falls 0.602 → 0.439, so its area falls 0.98 → 0.63 cm². The ray ends on the
  velopharyngeal port window, 1.4 mm (child) behind the roof's first point, the uvula's tip. So
  `on_roof` still fades the contact model in, at 0.65. The model takes the narrowest tongue gap
  among roof points ±`half` along the roof, but skips that search when `dc − half > 0.6k`. The
  shortcut assumes a neighbour can be closer only by `half`, which holds only for a ray ending on
  the roof. For a ray ending off it, a neighbour can be closer by `half + pr.dist`. Here the search
  switched on with a neighbour at 0.35 cm, inside the contact range: `cap` 0 → 0.60 in one step.
- Change: `crates/chimera/src/anatomy/human.rs`.
  - Experiment 11's patch as logged: `AreaLaw.ahead = (1.6, 1.4)`, faded in by
    `smoothstep(0, 0.4·k, mid.x − tip.x)`.
  - The roof-contact search's bound is now `dc − half − pr.dist ≤ 0.6k`. With it, a ray whose
    search is skipped has every neighbour farther than 0.6k, so `cap` and `w` are 0 either way
    and the contact fades in continuously. The fix leaves the calibrated vowel table
    byte-identical: no calibrated posture sits in the band it changes.
  - Tables regenerated with `chimera calibrate vowels all` and `chimera calibrate consonants`,
    the same postures as experiment 11's (child /ə/ identical).
  - Values: α = 1.6 only. α = 1.2 was not retried. In experiment 11 it put note MCD at
    8.648 dB, over the 8.636 dB guard, and the jump fix doesn't move the tables. It needs its own
    run, with something to offset the MCD.
- Speech: word error rate 40.73 → 36.09 % (training, **BETTER**), 50.67 → 45.67 % (held-out).
  Experiment 11 read 35.76 %; the difference is within Whisper's run-to-run spread.
  - Moved right: "chicken leg" (male, 5 → 1 errors), "juice" (male), "sharp" and "the thin road"
    (female, also male 3 → 2), "soft hill" (female), "some bright doors" (male), "birch canoe"
    (female, 7 → 6), "rice … round bowls" (male, 2 → 1).
  - "Sheet" is heard in the male voice ("screw the sheet": the error moved to "glue"). The
    female still hears "feet".
  - One got worse: "her round table sings the dark rope" (female, 3 → 4, "the" dropped).
  - Chair, chain, chopped and the female birch still fail.
- Singer: training 0.582 → 0.584 (NO CLEAR CHANGE). Every guard holds: 0 notes off time, 0
  dropped gestures, note MCD 8.536 → 8.628 dB, envelope correlation 0.811, pitch error 4.85
  cents. Held-out 0.789 → 0.810. Level 0–0.5 kHz cost 1.22; nothing else moved by more than
  0.02. The same as experiment 11.
- Speed: `os=2` 18.2 % (baseline) → 18.4 %, at load average 10.
- Tests: fixtures blessed twice, then `cargo test --workspace`. Everything passes, including
  `geometry_continuity`, `golden_geometry` and `golden_planning`, except one:
  - `golden_analysis` fails at its first "human |H|" check ("male neutral", 9 %): the README's
    airway exception, since the tract geometry changed on purpose.
  - With the human checks skipped in a local, uncommitted edit, the rest of `golden_analysis`
    passes (tube networks, FFT, LPC, YIN, Nelder–Mead).
  - `bun run wasm && bun test tests`: 30 pass.
  - Fixtures restored.
- Commit: 68f6274.
- Next:
  - The /ʃ/'s level: the female /ʃ/ is about 21 dB below its vowel, as quiet as her /f/, so she
    still says "feet". Look at the teeth (obstacle) source's gain, and at how its weight
    falls with the distance behind the teeth: /ʃ/ is at 2.5 cm, just short of where `w_dist`
    starts to fade, at 2.6.
  - Then α = 1.2, which moved /ʃ/ further in experiment 11.
  - The affricates (chair, chain, chopped, birch) still fail: their /ʃ/ release is presumably as
    weak.

### 13. The teeth (obstacle) source makes sibilants louder than /f/ (reverted)

- Hypothesis: the female /ʃ/ of "sheet" is 22 dB below its vowel, and Whisper hears "feet". A
  probe of `update_noise` and of the levels (frication 0.12–0.22 s against vowel 0.25–0.31 s, by
  RMS) found:
  - **`w_dist` is not the cause.** The female /ʃ/'s jet sits 2.49 body-cm behind the teeth and
    the male's 2.6. Both get full weight (`w_dist` 1.00 and 0.99–1.00), and so does `w_jet`.
    Moving the teeth injection point by −2 to +1 sections changes the level by less than 1 dB.
  - **In both voices /ʃ/ is no louder than /f/.** Male: /ʃ/ −11.5, /f/ −12.4, /s/ −9.3 dB re
    the vowel. Female: −22.2, −23.1 and −20.2. In real speech the sibilants are about 10 dB
    more intense than /f/ and /θ/ (Behrens & Blumstein 1988; Jongman et al. 2000), because the
    jet hits the incisors (an obstacle source, Shadle 1985). The obstacle source adds only
    9–10 dB over the jet source alone, in both voices (teeth source off: male −20.7, female
    −32.1).
  - **Why the female is about 10 dB worse than the male, relative to her vowel.** The female
    voice's output gain (0.55, `HumanVoice::new`) explains most of the 8 dB her frication is
    below the male's in absolute terms. It is a level match, so it doesn't change the
    fricative-to-vowel ratio. The ratio's gap splits about evenly:
    - The source: the female voice rendered in the male body gives /ʃ/ −17.1 dB. Her vowels
      come out 5.4 dB louder than the male voice's in the same body. Her glottal pulse is
      0.17·k² cm² (0.81 of the male's) at twice the f0. Real female peak flow is about 0.64 of
      the male's (Holmberg et al. 1988), with folds about 0.67 of his length (Hirano), so her
      vowels are too loud.
    - The body: the scale alone (0.9, other female body parameters set to the male's) puts the
      female frication 8 dB lower in absolute terms. After the 5.2 dB output gain, about 3 dB of
      that is the turbulence source itself. The source scales with Re² ∝ v²A, and the series
      pressure source's flow scales with A. Her vowel rises about 2 dB.
  - The cheapest step that separates /ʃ/ from /f/ is the obstacle source's gain. It raises every
    sibilant (and tongue-tip releases) in both voices and leaves /f/ alone, since /f/'s jet is
    at the lips (`w_dist` 0).
- Change: `NoiseGains.teeth` 3.0e-6 → 6.0e-6 in `crates/chimera/src/voice/human_airway.rs`.
  Values tried, by level re the vowel:
  - 6e-6: /ʃ/ male −5.8, female −16.4 dB (6–7 dB above /f/); /s/ male −3.4, female −14.4.
    Female "chair": the /tʃ/ peaks about 20 dB below the vowel.
  - 9e-6: /ʃ/ −2.3 and −12.9 dB, /s/ male +0.1 dB, as loud as the vowel. Too loud. Not judged.
  - The judges ran on 6e-6.
- Speech: word error rate 36.09 → 36.75 % (training, NO CLEAR CHANGE), 45.67 → 44.67 %
  (held-out).
  - Moved right:
    - **"Glue the sheet" (female, 1 → 0: "feet" → "sheet")**
    - "chopped" (male, 3 → 1)
    - "pushes" (male, 3 → 0)
    - "thrown" (female, 1 → 0)
    - "faced us" (male, 7 → 5)
    - "rice … served" (female, 3 → 1)
    - "the quiet fish" (female, 3 → 2)
  - Moved wrong:
    - "chicken leg" (male 1 → 5, female 2 → 4)
    - "some bright doors sell" (male, 3 → 6)
    - birch, "depth of a well", "late song", "sharp tree" and "loud milk", each by one word
  - Chair, chain and the female juice still fail.
- Singer: training 0.582 → 0.601 (+0.019 like for like), but **WORSE**: note MCD 8.536 →
  8.644 dB, over its 8.636 guard. The best run's 8.628 left 0.008 dB of headroom. The songs'
  sibilants are louder: level 4–8 kHz gap/spread 1.52 → 1.63, 2–4 kHz 0.96 → 1.00. Every
  other guard holds (0 notes off time, 0 dropped gestures, envelope correlation 0.811, pitch
  error 4.85 cents). Held-out 0.810 → 0.827.
- Speed: `os=2` read 27.8 and 28.3 % at load average 15–16. Back to back on the same loaded
  machine: 24.3 % without the change, 24.2 % with it. The change is a constant, so the speed
  is unchanged.
- Tests: fixtures blessed twice, then `cargo test --workspace`.
  - Everything passes except `golden_analysis` at "male neutral" (9 %). It is the same failure
    experiment 12 logged: that change altered the tract geometry, which the README's airway
    exception covers. This change doesn't cause it.
  - `bun run wasm && bun test tests`: 30 pass.
  - Fixtures restored.
- Verdict: reverted. The speech isn't BETTER (+0.66 points), and the singer breaks the note-MCD
  guard.
- Commit: none (log only).
- Next:
  - The sibilant level is still the right target. The female "sheet" is heard as soon as /ʃ/
    stands 6 dB above /f/. But a flat gain also makes the songs' sibilants louder, and note MCD
    has no headroom left (8.628 against 8.636).
  - Take the female's 10 dB first. Her vowels are about 5 dB too loud for her folds: the glottal
    pulse area scales with k² (0.81), where real female folds are about 0.67 of the male's
    length and her peak flow about 0.64 of his. A fold-length body parameter for the pulse
    amplitude, with the output gain re-levelled, would lift every female consonant against
    her vowels without touching the male. It does change her source, so watch H1−H2 and MCD in
    the singer.
  - If the gain is retried, try a smaller step (4–4.5e-6) together with something that buys back
    note MCD. Or tie the obstacle source to Stevens' scaling (source ∝ ΔP^1.5·A^0.5 rather than
    Re² ∝ ΔP·A), which lifts narrow jets and small bodies more than wide ones.
  - "Chicken leg" and "some bright doors" got worse with louder /tʃ/ and /s/: check the /tʃ/
    release, which may now mask the /k/ after it.

### 14. A voiceless fricative's glottal opening ends voicing at its onset, not 35 ms before (reverted)

- Hypothesis: target 2 (stressed vowels too short). Printing the planned segments
  (`plan_phrases`, as `render text` plans them) showed the duration rules are not the cause:
  - Stressed vowels are planned at Klatt-like values in connected speech: the /ʊ/ of "pushes"
    121 ms (against 66 ms for the "-es"), "eats" /i/ 93, "sweet" /i/ 110, "paints" /eɪ/ 158,
    "steady" /ɛ/ 123, "reads" /i/ 119.
  - The loss is in the realisation. In the male "pushes" the /ʊ/ is planned 0.847–0.968 s but
    voices only from 0.90 to 0.93 s. The /p/'s aspiration takes the first 50 ms, and voicing
    stops 35 ms before the /ʃ/.
  - The voiceless-fricative abduction gesture (`plan_utterance`, `Gesture::new(t0 − 0.015, …,
    ramp 0.03)`) starts opening the folds 45 ms before the fricative and is fully open 15 ms
    before it.
  - The same costs every /ə/ of "the" before s, ʃ, θ or f: it is planned at 68 ms and keeps
    about half.
  - The fix tried: after a voiced segment, the folds open late in it and are fully apart early
    in the frication (Löfqvist & Yoshioka 1980), so voicing ends near the fricative's onset.
    After a pause or a voiceless segment they are already apart, so that timing stays.
- Change: `crates/chimera/src/phonetics/planner.rs`. After a vowel or a voiced consonant, the
  "voiceless" abduction gesture starts at `t0 + lag` instead of `t0 − 0.015`. Values tried:
  - lag +10 ms: the /ʊ/ of "pushes" voices 0.90–0.955 s (55 ms, up from 30), and frication
    starts at 0.96 s (planned 0.968). Speech **WORSE**: 36.09 → 42.05 %.
    - "Rice is often served" (male, 1 → 6: "Her eyes is often turned").
    - "chicken leg" (male 1 → 5, female 2 → 4).
    - "faced us" (both voices).
    - Male "sheet" → "feet" again.
    - Only "chopped" (male) moved right.
  - lag 0 ms (fully open at the onset): NO CLEAR CHANGE, 36.09 → 38.08 %.
    - Moved wrong: "the thin road" (female 2 → 5, "the fin rolled"), "chicken leg" (male 1 → 3),
      "quiet fish" (male), "faced us" (female).
    - Moved right: "birch canoe" (male 6 → 5), "rice … bowls" (female 3 → 2).
  - The trend runs one way: the later the folds open, the worse the result (−15 ms, the current
    value: 36.09 %; 0: 38.08; +10: 42.05). The voicing carried into the fricative onsets makes
    s, ʃ, θ and f weaker, or heard as voiced.
- Speech: training 36.09 → 38.08 % (lag 0) and 42.05 % (lag +10). The held-out rate also
  changed, but it isn't used.
- Singer, bench, tests: not run. The deciding verdict failed at both values.
- Verdict: reverted. The speech is not BETTER at either value.
- Commit: none (log only).
- Next: see lead 7.
  - Don't retime the fricative abduction: the early opening helps the fricative more than the
    vowel loses.
  - What is still open on stressed-vowel length:
    - The aspiration allowance: a stressed vowel after a voiceless stop gets 20 ms back for a VOT
      of about 50 ms. Klatt's rule adds 25 ms, but his aspiration is shorter.
    - The stop-closure lead: closures land about 17 ms early.
    - Lengthening the stressed vowels in the plan directly. None of these opens the fricative's
      onset.

### 15. An affricate's frication lasts until the folds close, with no [h] after it (reverted)

- Hypothesis: the affricates fail in Whisper ("chain" → "king"/"pin", "birch" → "bird",
  "chair" → "coloring"/"scheduling", "juice" → "dose", "chicken" dropped). Renders of "chain",
  "chair", "birch", "juice" and /ˈɑtʃɑ/ in both voices showed why for /tʃ/:
  - The planner splits the affricate's span at 55 %: a postalveolar closure, then the /ʃ/
    constriction to the span's end. In /ˈɑtʃɑ/ the span is 67 ms (Klatt's 70/50 ms), so the
    closure gives about 20 ms of silence and the frication starts at about 0.29 s.
  - The /ʃ/ gesture is built with the affricate's own definition, a closing one, so
    `oral_gestures_into` takes a contact release's 20 ms latency lead off its end. The
    constriction lets go about 20 ms before the span ends.
  - The folds, meanwhile, get a voiceless stop's aspiration gesture: wide (0.36) until 20 ms past
    the span's end, then a 45 ms ramp. So the release is followed by only 35–40 ms of /ʃ/
    frication (0.29–0.33 s, peak 2.2–2.3 kHz, as loud as the /ʃ/ of /ˈɑʃɑ/), then about 50 ms
    of [h] (0.33–0.38 s, energy below 1 kHz) before voicing. The same in the female "chain"
    (frication 0.13–0.18 s, then [h] to 0.23). Heard as a stop and an [h]: "king", "pin".
  - In a real /tʃ/ the frication is the voice onset time: the glottis is open behind a held
    postalveolar constriction, and voicing starts as the frication ends, with no aspiration
    after it.
- Change: `crates/chimera/src/phonetics/planner.rs`. A voiceless affricate's /ʃ/ gesture is
  held to the end of the aspiration gesture's hold (`t1 + 0.4·VOT`), with the 20 ms latency lead
  added back, since a fricative constriction lets go at once. A voiced affricate's is held to
  `t1`. The VOT and the aspiration test became two small helpers shared with the stops. The vowel
  onset (voicing) doesn't move; the [h] becomes frication. Values tried:
  - **A**, held to `t1 + 0.4·VOT` (lead compensated): /ˈɑtʃɑ/ male frication 0.29–0.36 s at
    −29 to −31 dB, then about 10 ms to the vowel; female "chain" 0.13–0.21 s, a clean /ʃ/
    spectrum (2–4 kHz on top), at about −37 dB against the vowel's −17. Speech 36.09 → 37.75 %
    (NO CLEAR CHANGE). Right: "the black chain" (male), "chopped" (male, 3 → 1). Wrong: "chicken
    leg" (male 1 → 5, female 2 → 5, "These taste like a rare dish"), "juice" (female 2 → 3).
    The stressed /ɪ/ of "chicken" kept about 20 ms of voicing before; with the constriction
    held into its onset it lost it.
  - **A + the aspiration allowance for affricates** (`durations.rs`: a stressed vowel after a
    voiceless affricate gets the 20 ms that one after a voiceless stop gets, since the same
    abduction gesture eats its onset): speech 36.09 → **37.09 %** (training, NO CLEAR CHANGE),
    45.67 → 48.33 % (held-out). Right: "chain" (male 4 → 3), "chicken leg" (female 2 → 1). Wrong:
    "chicken leg" (male 1 → 5), "juice" (female 2 → 3, "roaster"). Chair, the female chain
    ("fin"), birch and the male chopped are as before.
  - **B**, held to `t1 + 0.4·VOT` without compensating the lead (checked by frames only): the
    [h] comes back (/ˈɑtʃɑ/ 0.34–0.38 s), and voicing in "chicken" starts no earlier. Voicing
    onset follows the folds, not the tongue. Not judged.
  - "Juice": the /dʒ/ barely changes under either. Its frication (0.10–0.14 s) is about 25 dB
    below the vowel with a flat spectrum. It is quiet, not mistimed.
- Speech: training 36.09 → 37.09 % (A + allowance) and 37.75 % (A). Held-out not used.
- Singer, bench, tests: not run. The deciding verdict failed at both values.
- Verdict: reverted. The speech is not BETTER at either value.
- Commit: none (log only). The patch (A + allowance), for a retry:
  - `planner.rs`, `Manner::Affricate`: `let open = if def.voicing == Voicing::Voiceless { 0.4 *
    stop_vot(next_vowel, prev_seg, rate) } else { 0.0 };` and the release gesture
    `add_oral(&rel.targets, split + 0.008, t1 + open + 0.02, 1.4, def.ipa)`. Here `stop_vot` is
    the stops' `if aspirated { 0.05 / rate } else { 0.012 }` with
    `aspirated = next_vowel && !prev.is_some_and(|p| p.ipa == "s")`.
  - `durations.rs`: the +20 ms test becomes `matches!(pv.manner, Some(Manner::Stop |
    Manner::Affricate)) && pv.voiced != Some(true)`.
- Next:
  - The /tʃ/ now has the right shape in the renders, and Whisper hears it in some places (the
    male "chain"). What holds it back is level: the female /tʃ/'s frication sits about 20 dB
    below her vowel, like her /ʃ/ (lead 6, experiment 13). Retry this patch on top of a fix for
    the female fricative level (the glottal pulse amplitude by fold length), not on its own.
  - The male "chicken" is a short-vowel problem (lead 7): its stressed /ɪ/ voices for only about
    20 ms between the affricate's VOT and the /k/ closure's lead.
  - /dʒ/ is a separate fault: it is quiet, and it has no abduction at all, where a voiced
    fricative gets 0.035. Look at its release's glottal setting and level, not its timing.

### Decision after experiment 15: the pop gets its own keep rule

The owner asked for the pop on "sharp" to be fixed, but a single pop can't lower the training
word error rate by the 1.5 points that batch 2's keep rule asks for. So a change aimed at target 3
is kept when the ignored test `the_p_of_sharp_does_not_pop` passes (and is un-ignored in the same
commit), speech is not WORSE, and the singer guard holds. See the README.

Experiments 13 and 15 both point at the female fricatives' level (lead 6) and both run into the
singer's note-MCD guard, which experiment 12 left with 0.008 dB of room. Whether to widen that
guard for this batch is the owner's call; until then, experiments avoid changing the female
voice's sung consonants.

### 16. A pre-placed closure behind a stop seals as the front one opens: the pop on "sharp" (kept)

- Hypothesis: target 3, under its own keep rule. A trace of the male "sharp tree" (/ɹ/
  2.063–2.123 s, /p/ 2.123–2.188, /t/ 2.188–2.250) found the following:
  - The lips seal at about 2.130 s.
  - The tongue seals for /t/ at 2.169 s, 19 ms before its planned closure. The lips open at
    2.1925, so a pocket is trapped for 23 ms.
  - The /t/'s latency lead (`early`, 31 ms for the tip) assumes a tip rising from a vowel.
    Here the tip comes from the /ɹ/ posture (TtX 5.35, TtY 9.35), already close to the ridge,
    so it reaches contact early.
  - The pressure was already 5.5x the lungs' at the seal. In the last 30 ms before it, the
    swept volume ahead of the tongue was about 4.6 cm³, pushed back through a gap closing to
    zero:
    - tip forward 2.5 cm³
    - lips un-protruding 1.5 cm³
    - jaw 1.5 cm³
    - curl unwinding 1.0 cm³
  - After the seal the pocket shrank 6.9 → 5.4 cm³ (the jaw rising 1.17 → 0.75 cm alone
    accounts for −1.1 cm³), taking the pressure to 10.5x.
  - In a trapped pocket of a few cm³, any of these motions overshoots 2x the lungs'. So the
    model can't stand an early seal at all. What it can do is seal when the lips open.
- Change: `crates/chimera/src/phonetics/planner.rs`. A stop that closes behind the stop before
  it (front-to-back place order, `closes_in_front`) is aimed `PLACED_CLOSURE_DELAY` = 25 ms later
  when its dominant articulators were set by the consonant before that stop, or by a rhotic
  vowel (`pre_placed_behind_closure`). After a plain vowel the timing is unchanged. The test
  `the_p_of_sharp_does_not_pop` is un-ignored. Values tried, as a delay on every front-to-back
  stop cluster at first (p/lung, then the first stop's release in dB re the loudest vowel, male):

  | delay | /ʃˈɑɹptɹˈi/ | /ˈɑptɑ/ | /ˈɑpkɑ/ |
  |---|---|---|---|
  | 0 (before) | 10.5x, +7.0 | 2.1x, −7.9 | 4.0x, +1.5 |
  | 15 ms | 4.5x, +6.5 | 2.2x, −4.5 | 3.2x, −0.9 |
  | 25 ms | 1.3x, −6.5 | 2.2x, −2.5 | 3.2x, −0.2 |
  | 35 ms | 1.4x, −7.0 | 2.1x, −1.8 | 3.0x, +0.3 |

  - A blanket delay makes the /p/ of "apt" louder: from a vowel the /t/ already seals as the
    lips open, so delaying it releases the /p/ into an open tract. So the rule is limited to a
    pre-placed articulator.
  - A first version counted any vowel as placing the articulators. A vowel always sets the
    tongue body, so that also moved /pk/ and /tk/; it was narrowed to consonants and rhotic
    vowels.
  - With the rule, /ˈhɛlpt/ reads 1.2x, −12.3 dB and /ˈɝptɑ/ 2.97x, +5.4 → 2.05x, −4.8 dB.
    /ˈɑptɑ/, /ˈɑpkɑ/ and /ˈɑtkɑ/ are as before.
- The /p/ of "sharp tree" (the test): male +6.9 → **−6.8 dB**, oral pressure 10.5x → **1.3x**;
  female −8.0 → −17.6 dB, 10.5x → 1.3x. The p's of "paper" sit at −6 and −7.
- Speech: word error rate 36.09 → 36.09 % (training, NO CLEAR CHANGE), 45.67 → 45.67 %
  (held-out). No word moved. Whisper hears "sharp tree" right in both voices, before and after
  ("The coated paper locks under the sharp tree.").
- Singer: training 0.584 → 0.584 (baseline 0.582, NO CLEAR CHANGE). Every guard holds: 0 notes
  off time, 0 dropped gestures, note MCD 8.628 dB (unchanged), envelope correlation 0.811,
  pitch error 4.85 cents. Held-out 0.810 → 0.810. No sung cluster in the training songs seems
  to meet the rule, or none that changes a scored term.
- Speed: `os=2` 18.4 % (baseline 18.2 %), at load average 10–17.
- Tests: fixtures blessed twice, then `cargo test --workspace --release`.
  - Everything passes, `the_p_of_sharp_does_not_pop` included, except `golden_analysis` at
    "male neutral" (9 %). That is experiment 12's geometry change (the README's airway
    exception); this change touches only the planner.
  - `bun run wasm && bun test tests`: 30 pass.
  - Fixtures restored.
- Verdict: kept under target 3's rule. The test passes, speech is not WORSE, and the singer
  guard holds.
- Commit: 8ebb020.
- Next:
  - Other front-to-back clusters still pump, from the same physics: the second constriction
    narrows to closure with the front one shut.
    - /ˈɑpkɑ/: 4.0x, /p/ at +1.5 dB (male).
    - /ˈɑbdɑ/: 6.4x, /b/ at +1.9. The folds are adducted, so the pocket runs back to the
      glottis.
    - /ˈɑtkɑ/: 3.1x.
  - None of these is pre-placed, so a delay only trades the pump for a released first stop.
    The model's articulators keep moving at full speed into a sealed or nearly sealed space.
    A real speaker's are slowed by the rising pressure, and the cheeks and lips yield. Two
    candidates: a consonant jaw target (alveolars and velars have high jaw positions, so the
    jaw would be up before the second closure), or a lip seal that leaks above a contact
    pressure (the README's "real leak path"). Both reach every stop, so watch the singer's
    note MCD.

### 17. Tense vowels heard as lax: what cuts them, and an aspiration allowance as long as the VOT (reverted)

- Hypothesis: "eats" → "it's", "sweet stone" → "switch tone", "paints" → "pinks" and "reads" →
  "greater", in both voices: the tense vowels are heard as lax ones. Experiment 14 left two
  candidates, the realisation cutting the vowel (stop closures early, aspiration longer than its
  allowance) and the vowel undershooting its target. Measured first (male, LPC formants and
  10 ms levels; a Whisper probe with the judge's settings on the six failing nonsense sentences):
  - **The closure lead depends on the vowel, but it isn't what loses the /i/.** The /t/ of
    /ɑhˈVtə/ reaches −30 dB re the vowel's peak, against its planned onset: i −7, ɪ −4, e −3,
    u 0, o +3, ɛ +7, ə +9, æ +14, ɑ +23 ms. The tip's 31 ms latency lead is fixed, but its travel
    to the ridge is 2.4 cm from /i/ and 4.2 cm from /ɑ/ (vowel-table ttY against the stop's 9.68).
    Aiming the closure by travel (16.5 ms per cm round 2.8 cm) moves /ɑ/ from +23 to −5 ms but /i/
    only from −7 to −5: after /i/ the level falls 8–9 dB in the 20 ms before contact (F1 370 →
    310 Hz) as the tip narrows an already narrow front tract. Aiming it later only (up to 21 ms
    for /i/) moved no word in the probe.
  - **The /i/ undershoots, mostly from its neighbours.** "sweet" (/i/ planned 1.682–1.792 s): F2
    reaches 2200 Hz (table 2225) only from 1.725 s, about 35 ms before the level falls for the /t/;
    the /w/ owns the tongue body until its end, then the 12 Hz dynamics take 40 ms. F1 stays at
    390–400 (table /i/ 337, /ɪ/ 424) and F3 at 2550–2700 (/i/ 2901, /ɪ/ 2603). "river eats"
    (/i/ 0.937–1.030 s, after /ɝ/): F1 about 445, F2 1650 → 2080 only as the /t/ closes. It never
    reaches /i/. This /i/ also takes `klatt_durations`' vowel-after-vowel factor (0.7), which
    applies across the word boundary.
  - **"paints"**: the /p/'s aspiration takes 1.05–1.10 s, and the /eɪ/ (planned 1.053–1.211) is
    voiced for about 70 ms, from 1.10 s to the nasal murmur at 1.17. Its /e/ sits at 465/2030 Hz,
    the table's /e/, which is only 40 Hz of F1 from /ɪ/ (424/2031), as in Hillenbrand's data. What
    separates them is length and the glide.
  - **Length is the lever.** Adding 30 ms to every stressed tense vowel (a diagnostic, not a
    rule) makes "paints" heard right in both voices, and the male "sweet stone" too.
- Change: `crates/chimera/src/phonetics/durations.rs`, the aspiration allowance (a stressed vowel
  after a voiceless stop gets back the time the aspiration eats). It is 20 ms, against the
  planner's 50 ms VOT (the abduction gesture holds to `t1 + 0.4·VOT`, then ramps for 45 ms, and
  voicing starts about 50 ms after the release). Sung durations are explicit, so this is speech
  only. Values tried, full speech judge:
  - **Diagnostic, stressed tense vowels +30 ms**: 36.09 → 37.09 % (NO CLEAR CHANGE).
    - Right: "paints" and "sweet stone" (male 4 → 2), "paints" (female 5 → 4), male "Glue",
      female "rice … served" (3 → 1) and "thrown", male "chair" (3 → 2).
    - Wrong: male "chicken leg" 1 → 7 (Whisper stops after "These days,"), "the thin road" (male
      2 → 4, female 2 → 3), female "birch" and "steady".
  - **Allowance 20 → 50 ms**: 36.09 → 36.09 % (NO CLEAR CHANGE).
    - Right: male "paints", female "thrown" and "a large size", male "steady" (7 → 6).
    - Wrong: female "sharp tree" ("tray"), male "pushes a heavy" ("has a"), female "Four hours"
      (6 → 7: Whisper stops at "For our L").
  - **Allowance 50 ms, and only where the planner aspirates** (not after /s/, as in "steady" and
    "stone", whose /t/ is unaspirated but got the allowance): 36.09 → 36.09 % (NO CLEAR CHANGE),
    held-out 45.67 → 44.67 % (not used). "paints" is heard right in both voices. Otherwise as the
    row before.
- Speech: training 36.09 → 36.09 % at best (the last two rows); held-out not used.
- Singer, bench, tests: not run. The deciding verdict failed at every value.
- Verdict: reverted. The speech is not BETTER at any value: each gain on a target word is paid
  for by a loss elsewhere. Some losses are Whisper stopping mid-sentence.
- Commit: none (log only).
- Next:
  - "paints" is solved by length: any allowance of 50 ms or more fixes it in the male voice, and
    in both when the allowance is limited to aspirated stops. Keep that variant in mind as a
    passenger for a change that moves more words. On its own it is a wash.
  - The /i/ of "eats" and "sweet" is lost to its neighbours, not to the stop: the /w/ and the
    /ɝ/ hold the tongue body until their ends, and the /i/ then needs 40 ms to arrive. Two
    untried, speech-only angles:
    - the vowel-after-vowel 0.7 factor in `klatt_durations` across a word boundary
      ("river eats"). Klatt has no such rule; a word-initial vowel isn't shortened by the
      word before it;
    - an earlier tongue-body release for /w/ and /ɹ/ before a front vowel (coarticulation: a
      speaker's tongue anticipates /i/ during the /w/ of "sweet").
  - "sweet stone" and "eats" also fail on the /ts/: with the /i/ fixed, the male hears "each"
    and "switch". A /t/ released into /s/ is heard as /tʃ/. Look at the /s/ after a /t/ release
    (its tip place and the frication's spectrum) as a separate lead.
  - The tip-travel closure lead is real (30 ms of spread from /i/ to /ɑ/) but moves sung
    consonants, so it's for after the owner's note-MCD decision.

### 18. Utterance onsets: speech's breath lead, then a click on every /f/ and /v/ (reverted)

- Hypothesis (the suggested lead): the first word goes wrong more often than chance in both
  voices ("Four hours" → "Before I was …", "A cold" → "The coated", "A large" → "Your large",
  "Her round" → "Roundtable"). Speech's breath still reaches full drive 20 ms before the first
  segment (experiment 10 moved only sung phrases), so the adducted folds may voice before the
  first segment is in place.
- What the renders showed (male and female, first 300 ms, with the planned segments; the
  speech judge's renders start their plan at 0.05 s, first segment at 0.11):
  - "A cold": the /ə/ voices from 0.08 s, 30 ms before its planned start. "Her round": [h] from
    0.06 to 0.21 s, then a voiced /ɝ/ of 80 ms that runs straight into the /ɹ/ of "round".
  - "Four hours" (both voices): a burst at 0.100 s, peaking at −15 dB (the vowel is −16), with
    its energy below 1 kHz, 10 ms before the /f/. It isn't voicing: the breath change below
    moved the voicing but not this burst. It is a click from the geometry. In "ɑfɑ" the same
    click peaks at −10 dB mid-word. Every /f/ and /v/ has it; /s ʃ θ h p/ onsets don't.
  - Its cause, from a per-millisecond probe of the tract (`HumanVoice::telemetry` and the
    frame's grid), is two discontinuities in the labiodental geometry (`anatomy/human.rs`):
    1. The lip-contact cap (`ap_eff`) switched off at labiodental 0.5 in one step. Below it,
       /f/'s lip aperture target (0.05) caps the front gaps, so on the way in the lips form a
       near-bilabial constriction (0.75 cm² at a phrase start), and at 0.5 it opens at once:
       a [b]-like release.
    2. The last front vertical ray stops meeting the lower lip once the lip tucks behind it
       (labiodental ≈ 0.53). `measure` then falls back to a floor 3.5 cm down: that sample's
       gap jumped 1.6 → 3.5 cm, its midpoint dropped 1 cm and the oral tract got 0.9 cm longer
       in one control step (7.36 → 8.28 cm).
- Change, tried in order (speech judge only; training lines read):
  - **Speech's breath timed like a sung phrase's** (`plan_phrases` reaching full drive at the
    first segment, live notes unchanged): 36.09 → 39.40 % (WORSE). No first-word error
    changed. Male "chicken leg" 1 → 5 and a few one-word shuffles. Reverted before the next
    try.
  - **The click, fixed with a flat or lip-face floor**: the cap hands over continuously
    (`ap + 0.3k·s/(1−s)`, `s` = smoothstep(0, 0.5, lab)), and a vertical in front of the
    lower lip's edge takes the lip's outer face (the outline's tangent, 0.3 : −0.4) as its
    floor. The click is gone, but a held /f/ now ends in a 0.47 cm² lip slit instead of the old
    10 cm² opening, and comes out ~5 dB quieter. 36.09 → 38.74 % (NO CLEAR CHANGE): male
    "Four hours" right ("for our steady work history", 7 → 5) and male "served", but "fine" →
    "hind", "fixes" → "hits" and "fed" → "head" (the weaker /f/ is lost).
  - **Floor slopes between the two** (probe only, male fɑ / ɑfɑ, onset peak and held /f/ in
    dB; original −15.2 / −10.4 and −36.2 / −33.4): slope 4 → −41 / −30 and −47 / −45;
    12 → −36 / −29 and −42 / −38; 20 → −31 / −24 and −39 / −37; 40 → −24 / −17 and −38 / −35.
    A steeper floor buys back the held /f/ only by bringing back the click.
  - **The floor opens with the tuck instead** (kept as the patch below): the lip-face floor
    blends to the old depth over labiodental 0.7 → 1, so a held /f/ keeps its open front. Probe:
    onset peak −37.0 / −29.9 dB (was −15.2 / −10.4), held /f/ −37.2 / −33.9 (was −36.2 /
    −33.4). Opening from 0.3 or 0.5 left part of the click (−20 and −27 dB at a phrase start).
    In the judge's "Four hours" (male) the burst at 0.09–0.10 s fell from −25 to −42 dB.
    Speech 36.09 → 37.09 % (NO CLEAR CHANGE). "Four hours" is still heard "Before I was …"
    (male), so that extra syllable wasn't the click. Moved: female "rice … served" (3 → 4),
    "juice" (2 → 3), "Four hours" (6 → 7), male "sell" right (3 → 2), "paints the sweet stone"
    (4 → 5).
- Speech: training 36.09 → 37.09 % at the last value (NO CLEAR CHANGE); held-out 45.67 →
  45.67 % (not used).
  - My first run's `tail` printed the held-out transcripts along with the training ones. I
    didn't use them for any choice; every later run printed training lines only.
- Singer, bench, tests: not run, since the deciding verdict failed at every value.
- Verdict: reverted. The speech is not BETTER: the breath retiming is WORSE, and the click fix
  removes a real artifact but doesn't move Whisper's total.
- Commit: none (log only). The patch, against 41e3777 (`crates/chimera/src/anatomy/human.rs`):
  - constants: `LAB_HANDOVER = 0.5`, `LAB_GAP = 0.3`, `LAB_FRONT_OPENS = 0.7`;
  - in the lip contact: `let lab_open = smoothstep(0.0, LAB_HANDOVER, lab); let ap_eff = if
    lab_open >= 1.0 { f64::INFINITY } else { ap + LAB_GAP * k * lab_open / (1.0 - lab_open) };`
    in place of `if lab > 0.5 { f64::INFINITY } else { ap }`;
  - in the front verticals' loop, `if let Some(mut m) = measure(...)`, then
    `if x > ll_edge.x { let face = js_min(ll_edge.y - (x - ll_edge.x) * 4.0 / 3.0, m.outer.y);
    let y_in = lerp(face, m.outer.y - 3.5 * k, smoothstep(LAB_FRONT_OPENS, 1.0, lab));
    m.inner = v(x, y_in); m.d = m.outer.y - y_in; }` before `grid.push(sample(m, 3))`.
- Next:
  - The /f/ click is an artifact for the ear: a burst at about vowel level on every /f/ and
    /v/, and in the songs too ("f" and "v" lyrics). Whisper doesn't penalise it, so like the
    pop it needs its own keep rule from the owner: a test (the onset peak of "fɑ" and "ɑfɑ"
    against the held /f/), with the singer guard and speech not WORSE. The patch above is
    ready for that. It moves the geometry snapshots; the held /f/ is within 1 dB, so no
    recalibration should be needed.
  - The first-word errors aren't the breath. "A" → "The"/"Your" didn't move when its voicing
    moved 20 ms later. Whisper may just prefer a determiner it knows for a short initial /ə/.
    "Her round" → "Roundtable" is an 80 ms /ɝ/ running into /ɹaʊ/ with no boundary between
    them: a lead for the /ɝ/ + /ɹ/ sequence, not the onset.
  - Don't retry the speech breath lead on its own.

### Decision after experiment 18: a test for the /f/ click, under the artifact rule

Experiment 18 found a click before every /f/ and /v/, as loud as a vowel. Whisper doesn't count
it, but a listener would, so it gets the same treatment as the pop on "sharp": an ignored target
test, `an_f_does_not_click` (the loudest 2 ms peak from 20 ms before the fricative to 10 ms into
it, over the fricative's median 2 ms peak; between vowels and sentence-initially, both voices;
fails above 6 dB). On the current engine it reads +8 to +20 dB; with experiment 18's patch
(`out/onset/exp18-labiodental.patch`, `LAB_FRONT_OPENS` 0.7) +0.3 to +4.6 dB. The README's
keep rule for target 3 now covers any artifact with such a test.

### 19. The labiodental geometry hands over without a step: no click before /f/ and /v/ (kept)

- Hypothesis (experiment 18 and the decision after it): two steps in the labiodental geometry
  (`anatomy/human.rs`) make a [b]-like click, about vowel level, before every /f/ and /v/. The
  lip-contact cap switched off at labiodental 0.5 in one control step, opening the near-closed
  lips at once; and the front verticals in front of the tucked lower lip fell back to a floor
  3.5 cm down, jumping the gap by about 2 cm and the tract's length by about 1 cm. Real lips
  don't do either: the lower lip leaves the upper one gradually as it slides under the incisors.
  Kept under the artifact rule, so the target is `an_f_does_not_click`, not the word error rate.
- Change: experiment 18's patch as it was (`out/onset/exp18-labiodental.patch`), in
  `crates/chimera/src/anatomy/human.rs`. `LAB_HANDOVER` 0.5, `LAB_GAP` 0.3, `LAB_FRONT_OPENS` 0.7;
  no constant was adjusted. The cap hands over as `ap + LAB_GAP·k·s/(1−s)`, `s` =
  smoothstep(0, 0.5, lab); a vertical in front of the lower lip's edge takes the lip's outer face
  as its floor and opens to the old depth from labiodental 0.7 to 1. `an_f_does_not_click` is
  un-ignored in the same commit.
  - The test: male ɑfɑ +3.3, ɑvɑ +4.6, "four hours" +0.3 dB; female +2.9, +3.9, +0.8 dB (limit
    6; +8 to +20 without the patch).
- Speech: word error rate 36.09 → 37.09 % (training, NO CLEAR CHANGE), 45.67 → 45.67 %
  (held-out). The same training moves as experiment 18's last value: male "sell" right (3 → 2);
  female "rice … served" (3 → 4), "juice" (2 → 3), "Four hours" (6 → 7, Whisper stops at "For
  our L"), male "paints the sweet stone" (4 → 5, "fast" → "first").
  - My first grep of the judge's output, and a first per-sentence diff that filtered on the
    wrong field, printed held-out transcripts along with the training ones. I didn't use them
    for anything; the lines above come from a diff filtered on `set == "train"`.
- Singer: training 0.584 → 0.589 (baseline 0.582: +0.007, within 0.05; NO CLEAR CHANGE).
  Held-out 0.810 → 0.805. Guards: 0 notes off time, 0 dropped gestures, note MCD 8.628 →
  **8.614 dB** (better, so the guard's small room isn't spent), envelope correlation 0.811, pitch
  error 4.84 cents. Terms that moved, all by 0.02 or less: level 0–.5k cost 1.23 → 1.24, H1−H2
  2.29 → 2.30, aperiodicity 0.05 → 0.07; held-out voiceless-initial onset 0.22 → 0.14.
- Speed: `os=2` 26.1 % on the first run at load average 16 (baseline file 18.2 %). Rerun back to
  back with the change reversed, twice: patched 27.9 / 23.0 %, HEAD 27.2 / 25.8 %. No slowdown;
  the change adds a few comparisons per front vertical per control step.
- Tests: fixtures blessed twice, then `cargo test --workspace --release --no-fail-fast`: 104
  pass, `an_f_does_not_click` and `the_p_of_sharp_does_not_pop` included. The one failure is
  `golden_analysis` at "male neutral" (9.2 %), experiment 12's geometry change; it reads the same
  9.2055 % with this change reversed. The slow `calibrate_vowels_reproduces_the_committed_table`
  (run with `--ignored`) passes, so the vowel tables need no recalibration. `bun run wasm && bun
  test tests`: 30 pass. Fixtures restored.
- Verdict: kept under the artifact rule. The click test passes, speech is not WORSE, the singer
  guard holds (note MCD slightly better), the bench is not slower and the tests pass.
- Commit: 5cf036b.
- Next:
  - Both artifact targets are closed. The front-to-back clusters (/pk/, /bd/, /tk/) still pump
    to 3–6x (experiment 16's "Next"); they have no target test yet.
  - The first-word errors and the /ɝ/ + /ɹ/ of "Her round" remain open (experiment 18's
    "Next").

### 20. The obstacle source's jet distance runs from the channel's exit: the female /ʃ/ before /ɑ/ (reverted)

- Hypothesis (the suggested lead: /ʃ/ is 2–4 dB quieter than /s/, where real /ʃ/ is the louder).
  A probe of `update_noise` during /ˈɑsɑ/ and /ˈɑʃɑ/ (0.30–0.36 s) found:
  - **The source is not weaker for /ʃ/.** Its constriction is wider (male 0.20 cm² against 0.09,
    female 0.17 against 0.07) and carries more flow (male 460 cm³/s against 280), so the teeth
    source's Re² term is about 1 dB *higher* for /ʃ/ than for /s/. `w_jet` is 1.00 for both in
    both voices.
  - **The female /ʃ/ before /ɑ/ loses 2 dB to `w_dist`.** Her postalveolar channel spans two
    sections (0.17, 0.19 cm²), and the narrowest is the back one, 3.02 body-cm behind the teeth,
    in the far fade (2.6–4.0): `w_dist` 0.78. The jet leaves from the front section, 2.4 body-cm
    behind. The male's narrowest section is the front one (`w_dist` 1.00), as are both /s/'s.
  - **The rest is the transfer.** With only the teeth source on, it makes all of the fricative's
    level (−27.9 dB male /s/ against −27.8 with every source on), and from the same source level the
    radiated /ʃ/ is 2.8 dB weaker than /s/ (male) and 2.1 dB weaker (female). A rough budget: the
    radiation's +6 dB/octave favours /s/'s 4.5–5 kHz peak over /ʃ/'s 2.3–2.9 kHz by about 6 dB,
    and the teeth source's 5 kHz two-pole low-pass takes back about 4.5 dB of that; the last dB
    or so is the resonances.
- Change: `crates/chimera/src/voice/human_airway.rs`, `update_noise`. The teeth source's
  distance to the teeth runs from the most anterior section of the channel still within a factor
  of the minimum area, not from the narrowest section. Values tried:
  - **A**, both fades from the exit, sections under 2× the minimum: female /ˈɑʃɑ/ −40.0 →
    −37.6 dBFS (+2.4 dB), /s/ and the male unchanged. But it also moves a dental /ð θ/'s
    reference onto the teeth (their channel of 0.17–0.22 cm² runs up to the incisors' edge), so
    the near fade turns off their obstacle noise: every "the" changes, and the female "the thin
    road" became "the fin rolled". Speech 37.09 → 39.07 % (training, NO CLEAR CHANGE), held-out
    45.67 → 43.0 %.
  - **B**, only the far fade from the exit (the jet's travel), the near (dental) fade on the
    narrowest section as before, sections under 1.5×: same /ˈɑʃɑ/ gain, dentals unchanged.
    Speech 37.09 → **39.07 %** (training, NO CLEAR CHANGE), held-out 45.67 → 45.33 %. The patch
    is in `out/e20/variant-b.patch` (gitignored).
  - In the sentence that matters, it does nothing: the female "sheet" /ʃ/ (before /i/, tongue
    fronted) sits 2.47 body-cm behind the teeth and already had full weight, as experiment 13
    found. It is still heard as "feet". The fix reaches only /ʃ/ next to a back vowel.
- Speech (B), the training sentences that moved: male "chicken leg" (1 → 3, "quick lick"),
  "rice … round bowls" (1 → 3, "a round bowl"), "thin road" (2 → 3), "bright doors sell" (2 → 3);
  female "thrown" (1 → 0), "large size" (1 → 2), "green chair … late song" (2 → 3, "red song");
  male "fast window" (5 → 4). None of them is a female postalveolar.
  - **Most of these moves are Whisper's chaos, not the sound.** Diffing the judge's renders
    against the best run's, frame by frame: the male "rice" differs by at most −72 dB (about 50 dB
    below the signal) and the male "fast window" by −108 dB (a few 16-bit steps), and Whisper still
    changed its words ("bowls" → "a round bowl", "first" → "fast"). Whisper itself is
    deterministic (the same file gives the same text twice). So a change that touches the noise
    sources anywhere can swing the training total by about ±2 points with no audible cause. The
    BETTER threshold (1.5 points) is inside that noise for any small change.
  - The judge's printed output listed the held-out transcripts, and my first `tail` of it showed
    them. I didn't use them; the sentence comparison above is filtered on `set == "train"`.
- Singer, bench, tests: not run. The deciding verdict failed at both values.
- Verdict: reverted. The speech is not BETTER at either value, and the change misses the female
  "sheet".
- Commit: none (log only).
- Next:
  - Don't retry the jet-exit distance on its own: it is right, but it only reaches /ʃ/ before
    back vowels. Fold it into whatever next lifts /ʃ/, as a part of the same physical change.
  - The /ʃ/-below-/s/ gap is in the teeth source's spectrum against the radiation, not in its
    level or weights. The source is white from 1.2 to 5 kHz in pressure, and the radiation tilts
    the output toward /s/. A real obstacle source's spectrum peaks near a Strouhal frequency of
    the jet, f ≈ St·v/D, and falls above it. With the probe's numbers, v/D is about 2.1 times as
    high for /s/ (31 m/s through 3.4 mm) as for /ʃ/ (22 m/s through 5.1 mm). A teeth source whose
    band follows the jet's v/D, with its power held to the same total (a lower peak concentrates
    the same power in a narrower band), would lift /ʃ/ at 2–3 kHz and leave /s/'s band about where
    it is now. It touches every sibilant, so watch note MCD (0.022 dB of room). Its power scaling
    needs a citation (Stevens 1998, ch. 8; Shadle 1985) before it's coded.
  - Given the ±2-point chaos, a lift aimed at the female "sheet" should be checked on that
    sentence's /ʃ/ level before spending a judge run, and its verdict read as noisy.

### Batch 2 result (experiments 11–20)

| | Baseline (4980e3b) | Best (experiment 19) |
|---|---|---|
| Speech, training word error rate | 40.73 % | 37.09 % |
| Speech, held-out | 50.67 % | 45.67 % |
| Singer, training / held-out | 0.582 / 0.789 | 0.589 / 0.805 |
| Note MCD (guard 8.636 dB) | 8.536 dB | 8.614 dB |
| The /p/ of "sharp" re the loudest vowel (male) | +7.0 dB | −6.8 dB |
| Click before /f/ /v/ over the fricative | +8 to +20 dB | +0.3 to +4.6 dB |
| Real-time load (`os=2`) | 18.2 % | 18.4 % (quiet-machine reading) |

- Kept: 12 (the narrow space in front of the raised tongue tip puts /ʃ/ at its real pitch; and
  the roof contact no longer jumps at the port window), 16 (a pre-placed closure behind a stop
  seals as the front one opens: no pop), 19 (the labiodental geometry hands over without a step:
  no click). 11 found 12's change and was reverted on a test 12 fixed; 13, 14, 15, 17, 18 and 20
  were reverted.
- The female /ʃ/, /tʃ/ and /dʒ/ are still about 20 dB below her vowels. Every fix tried (13,
  20) either broke the note-MCD guard or didn't move the speech total.
- The speech judge is noisier than its threshold (experiment 20): renders a few 16-bit steps apart
  moved Whisper's training total by about ±2 points, against the 1.5-point BETTER margin.
- Open for the owner: widen the batch's note-MCD guard (0.1 → 0.3 dB proposed), and double the
  training sentences to make the speech judge steadier.
- Listening page (shared with the owner directly; before = the baseline, now = the
  best run; four training and four held-out sentences, two songs).

### Batch review (Codex), and what changed after it

Three findings, all fixed in one commit:
- **An area jump late in the lip tuck** (experiment 19's floor in front of the lower lip blended
  toward its open depth before a ray reached the lip's edge): up to 1.27 cm² at lab 0.75–0.94.
  The opening now grows from the edge over 0.1 cm. `geometry_continuity` sweeps the tuck from
  /f/-like postures, which the vowel sweeps never reached. A held /f/ is 1.5–2.8 dB quieter.
- **Experiment 16's delay overran short sung stops** (a /t/ closure 23 ms into the next vowel at
  a quarter beat, 180 bpm). It is capped at the stop's length less 30 ms; `sing_fast` has the
  reproduction.
- **The speech judge printed held-out transcripts**, which experiments 18–20 then saw (they say
  so in their entries). It prints training ones only.

After the fixes: speech 36.75 % training, 44.33 % held-out; singer 0.587, held-out 0.811, note
MCD 8.614 dB. `out/hill/best` holds this run, and the listening page was rebuilt from it.

### Batch review, round 2

- **Fixed, confirmed:** the cluster delay keeps every gesture ordered and inside its stop for
  durations of 10 ms and up (velars included), and the speech judge prints training transcripts only.
- **Still open, not new:** with the lips protruded and narrowed, the first front ray (behind the
  lower lip's edge, outside the new ramp) switches to a higher crossing of the lip's curve as the
  lip tucks: 0.41 cm² at lab ≈ 0.86 (NEUTRAL, jaw 0.3, lipAp 0.05, lipProt 0.2, lipWidth 0.5). Main
  has the same jump (0.30 cm²); this batch made it larger. It is kept as the ignored test
  `geometry_continuity::no_area_jumps_as_a_protruded_lower_lip_tucks` (jumps up to 0.48 cm²) and
  is lead 10 for the next batch: the lip-curve crossing that rays behind the edge pick.
- **Declined:** a stop shorter than 10 ms still holds its closure past its end (the minimum hold
  in `oral_gestures_into`). It predates this batch, and no planner produces one (the sung floor is
  12 ms).

## A judge against real voices (2026-11-07, between batches)

The owner found batch 2 much harder to hear than batch 1 (now 3, before 1, can't tell 4, songs
can't tell). Whisper had called it 4 points better: one noisy number that says nothing about
which sound is wrong. The owner asked for structural fixes rather than judge-loosening (the
proposed wider MCD guard), and for a judge built on real voices.

`chimera compare tsp` ([../speech-comparison.md](../speech-comparison.md)) says the Harvard
sentences 23 real speakers read (the TSP speech database) and compares each sound with theirs.
No batch uses it yet: whether it decides the next batch, and Whisper guards, is the owner's call.

- **The score.** 1.583 on the training speakers' sentences, 1.706 held-out. A real speaker gets
  0.277. It scores consonant levels, sibilant centroids and timing; spectrum, formants, voice
  quality, voicing and intonation are reported as diagnostics.
- **Both batches gained on it.** 1.994 before batch 1, 1.797 after, 1.583 after batch 2.
- **The leads it gives, largest first:**
  - **Female sibilant levels and centroids.** /ʃ/ −23 dB re her stressed vowels against the
    women's −7; /tʃ/ −26 against −9. /s/ centroid 4.7 kHz against 7.4; /z/ 4.0 against 7.1. No
    batch has moved the levels.
  - **Short closures and affricates.** /k/ 0.7 of a mean phone against 1.1–1.2; /dʒ/ 0.5 against
    1.2–1.3.
  - **/ð/ and /v/** 7–11 dB too loud.
  - **The male /ʃ/ centroid**, overshot by batch 2 (3.48 kHz, men 3.13).
  - **A diagnostic:** the voice lacks 1–4 kHz presence after batch 1 (female 1–2 kHz −16.1 dB re
    0–8 kHz, women −10.8). That is a spectral shape to fix, not a tilt.
