# Hill climb: one measured step at a time toward a real voice

An agent improves CHIMERA's voice in small steps. Each run is one experiment: one hypothesis and
one change, measured against real voices and guarded so that nothing else gets worse. The judges
are real speakers ([speech-comparison.md](../speech-comparison.md)), a real singer
([csd-comparison.md](../csd-comparison.md)) and Whisper's word error rate. A change is kept only
if the judges call it better. Nothing reaches main until the owner has listened to the result and
merged it.

The log of every experiment is [log.md](log.md). Read it before you start.

## The current batch: real voices (batch 3)

Batch 1 tuned the singing toward the singer, and the owner heard it (7 to 0). Batch 2 let Whisper
decide: its word error rate fell 4 points, and the owner could hardly hear it (now 3, before 1,
can't tell 4). Whisper's one noisy number doesn't say which sound is wrong. The owner wants
structural fixes, not a judge loosened until a change passes. So:

- **The real speakers decide.** A change is kept only if `chimera compare tsp` says **BETTER**
  against the best run: the training score falls by at least 0.01, and no scored term breaks.
- **Whisper guards.** Against the batch's baseline, its verdict on the training sentences must
  not be WORSE (at most 3 points higher). Understanding is what batch 2 earned.
- **The singer guards.** Against the batch's baseline, no singer guard may break (notes off time,
  dropped gestures, note MCD, envelope correlation, pitch), and the training score may rise by at
  most **0.05**.
- **Fix causes.** Before you change anything, name the physical cause and what value the literature
  or anatomy gives it, with a source. A constant tuned until a term reads right is not a fix, even
  when the judge says BETTER. If the cause turns out to be a plain level error, say how you know.
- **Diagnostics advise.** The report's spectrum, formants, voice, voicing and intonation terms
  don't decide, and a change doesn't have to improve them. Give their group costs in the log entry
  when they move. The owner prefers the darker voice: don't brighten the default to please the
  voice terms.
- **Pops and clicks** stay with `crates/chimera/tests/consonant_artifacts.rs`, which runs with the
  tests and must keep passing. The report's worst paired tokens are listening targets: some are the
  aligner failing on the synth, so check one before acting on it.
- **Order matters.** A change that lifts every female consonant against her vowels also lifts the
  ones already too loud (/ð v/), and the verdict breaks a term whose cost rises by more than 0.5.
  If that blocks a real fix, fix what it trips over first, in its own experiment.
- **Targets**, from the baseline's report (details in log.md, "Batch 3 baseline"):
  1. The female sibilants: /ʃ tʃ/ 16–17 dB too quiet, /s z/ centroids at 4.0–4.7 kHz where
     women's are 7.1–7.4.
  2. /dʒ/ and /z/ too short, nasals too long.
  3. /ð v/ 7–11 dB too loud in both voices.
  4. Anything else with a large cost in `out/hill/best/tsp/report.md`.
- **Branch:** `t3/real-voice-hill-climb`.

## Before you start

- **Where.** Work in this worktree, on the batch's branch (above). Never commit to main.
- **Data.** The singer's dataset is in `~/datasets/csd/CSD` (`tools/fetch-csd.sh`). The speakers'
  is in `~/datasets/tsp` (`tools/tsp/fetch.sh`, and `tools/tsp/setup.sh` once for the aligner).
  Whisper needs `tools/asr/setup.sh` once.
- **The best run so far** is in `out/hill/best/`: `tsp/score.json` and `tsp/report.md` (the
  speakers), `score.json` and `report.md` (singer), `speech/wer.json` (Whisper) and `bench.txt`
  (speed). If it is missing, measure the current commit into `out/hill/best` the same way as below.
- **The batch's baseline** is in `out/hill/baseline/`, measured the same way at the commit the
  batch started from. The guards are judged against it, not against the best run. A small
  allowance per step would otherwise add up over a batch. Reset it when a batch is merged and the
  next one starts.

## One experiment

1. **Pick one hypothesis.** Read the log and the deciding judge's output: for batch 3,
   `out/hill/best/tsp/report.md`, largest costs first, and the targets above (for a singing batch,
   `out/hill/best/report.md`). Aim at something the log doesn't show as exhausted, or try a new
   angle on it. Before you touch code, write down in a sentence or two:
   - what is physically wrong, and your source for what it should be
   - what you will change
   - which terms should move
2. **Make the change.** Keep it small, readable and motivated by physiology, acoustics or
   phonetics. These are fair game:
   - the voice source (`crates/chimera/src/sources/`)
   - the voice (`voice/`)
   - the airway (`airway/`)
   - anatomy and articulation (`anatomy/`, `control/`)
   - the planners (`phonetics/`)
3. **Measure** (below). If the change adds a tunable value, you may try a few values. Log what
   you tried.
4. **Keep or revert** (below).
5. **Log it** and stop. One experiment per run. If an idea needs a large rewrite or more than
   about an hour, log it under "Leads" as a proposal and pick something smaller.

## What you may not change

- **The judges.** Don't touch any of these:
  - `crates/chimera-cli/src/compare.rs`, `compare/score.rs`, `compare/tsp.rs`
  - `tools/asr/`, `tools/tsp/`
  - the held-out songs, the dataset, the sentences
  - `crates/chimera/tests/consonant_artifacts.rs`

  The comparison sings with the settings `render song` uses: pressure 9, tension 0.55 and
  35-cent vibrato (`render_notes` in compare.rs). Change how the engine responds to those
  settings, not the settings.
- **The output.** No EQ, filter or effect that shapes the sound toward the singer's spectrum.
  Fix the cause in the source or the tract.
- **Special cases.** None for the dataset, one song, one measured value, or the female body
  alone. A body parameter is fine when it has an anatomical reason.
- **The singing convention.** `plan_song` puts the vowel on the beat and its consonants before
  it. Don't move consonants onto the beat to fit CSD's note starts. Voiced-consonant onsets are
  not scored for this reason.
- **The calibrated tables** (`crates/chimera/src/phonetics/*.generated.json`) by hand. If a
  change moves the vowels' formants, regenerate the tables with `chimera calibrate` and say so in
  the log.
- **The tests.** Don't weaken or delete one to let a change through. A failing test, other than
  a golden fixture (below), means the change is reverted.
- **The held-out songs, sentences and speakers as a guide.** Decide on the training verdict only,
  and don't choose changes by the held-out scores, read the held-out sentences' transcripts, or
  read the `held` part of the speakers' `score.json`. They are there to show whether a gain
  carries over.

## Measuring

```sh
cargo build --release -p chimera-cli
target/release/chimera compare tsp ~/datasets/tsp out/hill/try/tsp --against out/hill/best/tsp/score.json
~/.cache/chimera-asr/.venv/bin/python tools/asr/speech_wer.py out/hill/try/speech --against out/hill/baseline/speech/wer.json
target/release/chimera compare csd ~/datasets/csd/CSD out/hill/try --against out/hill/baseline/score.json
cargo run --release -q -p chimera --example bench > out/hill/try/bench.txt
```

These are batch 3's comparisons, cheapest first: the speakers against the best run (they decide,
about 2 minutes), Whisper and the singer against the batch's baseline (they guard, about 1–3
minutes each). If the speakers' verdict isn't BETTER, you can stop there and revert. A singing
batch judges the singer against the best run instead.

1. **Speakers.** The comparison ends with a verdict: BETTER, WORSE or NO CLEAR CHANGE, and the terms
   that broke. `out/hill/try/tsp/report.md` has the new score, terms and worst tokens.
2. **Whisper.** It ends with BETTER, WORSE or NO CLEAR CHANGE on the training sentences: WORSE past
   3 points higher than the baseline. It lists the sentences it got wrong, and reports the held-out
   sentences, which never decide. About ±2 points of a small change's total is noise.
3. **Singer.** The comparison ends with a verdict and the reasons when worse. Read its guards. For a
   batch that guards the singer, a broken guard fails, and so does a training score more than 0.05
   above the baseline. `out/hill/try/report.md` has the new score table.
4. **Speed.** The bench prints the real-time load. Compare its `os=2` line with
   `out/hill/baseline/bench.txt`, the batch's baseline. It is noisy and depends on what else
   the machine is doing, so if it reads slower, rerun it and the baseline's commit back to
   back before calling a change slower.

## Keep or revert

Keep the change only if all of these hold (batch 3):

1. The speakers' verdict against the best run is **BETTER**.
2. Against the batch's baseline, Whisper's verdict is not WORSE, the singer breaks no guard, and
   the singer's training score rises by at most 0.05.
3. The bench is no more than **10 % slower** than the batch's baseline.
4. **The tests pass.**
   1. Bless the golden fixtures in `crates/chimera/tests/golden`. They are snapshots of the
      sound (the `.f32` files), of the planners' output (the utterance, duration, phrase, song
      and live-note plans, the beat sounds' gestures and the beat player's output, in
      `planning.json`) and of the tract geometry (area functions, contacts and outlines, in
      `geometry.json`), and they change with any change to these. Run `CHIMERA_BLESS=1 cargo
      test --workspace --no-fail-fast` **twice**: `engine.rs` reads a fixture that
      `golden_planning` blesses, so its first run can fail. The beat notation's expectations
      are not blessed. Other geometry tests (`geometry_continuity`, `airway`) are real checks,
      not snapshots: if one fails, fix the cause as part of the change.
   2. Then run `cargo test --workspace --no-fail-fast` and `bun run test`. Every test must
      pass. (The human airway's transfer functions in `golden_analysis` bless with the rest;
      its other cases, the analysis code's own, don't, and must pass as they are.)
   3. Restore the fixtures with `git checkout -- crates/chimera/tests/golden`. They are
      blessed once per batch, when the batch goes to a PR, so the history doesn't carry a copy
      of them for every experiment. Until then the golden tests fail on this branch, which is
      expected.
   4. `git status` must show only your change: no test file edited.

**If kept:**

1. Commit the engine change (without the fixtures). The message gives the hypothesis, the
   change, and the numbers:
   - the speakers' score before → after, training and held-out
   - Whisper's word error rate before → after, training and held-out
   - training and held-out singer score before → after
   - the terms that moved
2. Replace `out/hill/best` with `out/hill/try`:
   `rm -rf out/hill/best && mv out/hill/try out/hill/best`.
3. Add the log entry, commit it, and push.

**If reverted:**

1. Restore the engine: `git checkout -- crates/`, and remove any new files the change added.
   Delete `out/hill/try`.
2. Add the log entry, commit it, and push anyway, so the next run doesn't retry the idea.

## The log entry

Add one entry at the end of `log.md`:

```markdown
### N. <short title> (kept | reverted)

- Hypothesis: what was wrong, why the change should fix it, and your source.
- Change: what changed, in which files; values tried.
- Speakers: score a → b (training), c → d (held-out); the terms that moved, and the diagnostics'
  group costs if they moved.
- Whisper: word error rate a → b (training), c → d (held-out).
- Singer: training score a → b, held-out c → d; the terms that moved.
- Verdict: the reasons, if reverted.
- Commit: the hash, if kept.
- Next: what this suggests trying, or not trying, next.
```

Update "Leads" at the top of the log when an experiment closes a lead or opens a new one.

The repo is public. Commit messages and the log hold no absolute paths, hostnames or links to this
machine: `git grep -nIE 'ts\.net|beelink|/home/|/tmp/claude'` before you push (this line matches itself).
