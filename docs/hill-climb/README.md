# Hill climb: one measured step at a time toward a real voice

An agent improves CHIMERA's voice in small steps. Each run is one experiment: one hypothesis and
one change, measured against a real singer (the Children's Song Dataset, see
[csd-comparison.md](../csd-comparison.md)) and guarded so that speech, the tests and real-time
speed don't get worse. A change is kept only if the judges call it better. Nothing reaches main
until the owner has listened to the result and merged it.

The log of every experiment is [log.md](log.md). Read it before you start.

## The current batch: pronunciation (batch 2)

Batch 1 tuned the singing toward the singer. The owner heard the gain, but found it incremental and
cared more about pronunciation. So batch 2 swaps the roles of the judges:

- **Speech decides.** A change is kept only if the speech verdict (Whisper's word error rate on the
  training sentences) is **BETTER**: at least 1.5 points lower than the best run.
- **The singer guards.** Against the batch's baseline, no singer guard may break (notes off time,
  dropped gestures, note MCD, envelope correlation, pitch), and the training score may rise by
  at most **0.05**. Singing is what batch 1 earned; don't spend it.
- **Targets**, from what the owner heard (details in log.md, "Listening result"):
  1. sh, ch and j are too quiet and too low: "sheet" is heard as "feet", and j sounds muted.
     Every ch and j word in the training sentences fails.
  2. Stressed vowels are squeezed: the "pu" of "pushes" lasts about 40 ms.
  3. The p of "sharp" pops. Between the closed lips and the tongue sealing for the following /t/,
     the tongue squeezes the shut-in air to 11x the lungs' pressure, and the /p/ releases 7 dB
     above the sentence's loudest vowel (male; the p's of "paper" sit at −6 and −7 dB). Measure
     it with `cargo test --release -p chimera --test consonant_artifacts -- --ignored
     --nocapture`; the batch PR un-ignores that test once it passes. Simply absorbing a sealed
     pocket's wall motion was tried: it halves the pop but breaks the adiabatic-compression test
     in `airway_adversarial.rs` and the creatures' parity. Look at the articulation instead (how
     far and how late the tongue moves with the lips shut), or at a real leak path. Male p
     releases are loud in general (about −5 dB in "apa"; female −13 dB).
  4. Anything else Whisper's errors point at: the training sentences' wrong words are listed by
     the speech judge.
- **Audible artifacts have their own keep rule.** Whisper can't count a pop or a click, and fixing
  one can't move the word error rate by 1.5 points. A change aimed at an artifact with a target
  test in `tests/consonant_artifacts.rs` is kept when that test passes (un-ignore it in that
  commit; this is the one test edit allowed), the speech verdict is not WORSE, and the singer
  guard above holds. Everything else (bench, tests) as usual. Targets: the pop on "sharp"
  (`the_p_of_sharp_does_not_pop`, fixed by experiment 16) and the click before /f/ and /v/
  (`an_f_does_not_click`, found by experiment 18).
- **Branch:** `t3/pronunciation-hill-climb`, stacked on `t3/consonant-fixes` (the fix for the
  buzz, and the Bright control).

## Before you start

- **Where.** Work in this worktree, on the batch's branch (above). Never commit to main.
- **Data.** The dataset is in `~/datasets/csd/CSD` (fetch it with `tools/fetch-csd.sh`). The
  speech judge needs `tools/asr/setup.sh` once.
- **The best run so far** is in `out/hill/best/`: `score.json` and `report.md` (singer),
  `speech/wer.json` (speech) and `bench.txt` (speed). If it is missing, measure the current
  commit into `out/hill/best` the same way as below.
- **The batch's baseline** is in `out/hill/baseline/`, measured the same way at the commit the
  batch started from. Speech and speed are judged against it, not against the best run. A
  small allowance per step would otherwise add up over a batch. Reset it when a batch is
  merged and the next one starts.

## One experiment

1. **Pick one hypothesis.** Read the log and the deciding judge's output: for batch 2, the
   sentences Whisper got wrong in `out/hill/best/speech/wer.json` and the targets above (for a
   singing batch, `out/hill/best/report.md`, largest costs first). Aim at something the log
   doesn't show as exhausted, or try a new angle on it. Before you touch code, write down in a
   sentence or two:
   - what is physically wrong
   - what you will change
   - which words or terms should move
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
  - `crates/chimera-cli/src/compare.rs`, `compare/score.rs`
  - `tools/asr/`
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
- **The held-out songs and sentences as a guide.** Decide on the training verdict only, and
  don't choose changes by the held-out scores or read the held-out sentences' transcripts. They
  are there to show whether a gain carries over.

## Measuring

```sh
cargo build --release -p chimera-cli
cargo run --release -p chimera-cli -- compare csd ~/datasets/csd/CSD out/hill/try --against out/hill/baseline/score.json
~/.cache/chimera-asr/.venv/bin/python tools/asr/speech_wer.py out/hill/try/speech --against out/hill/best/speech/wer.json
cargo run --release -q -p chimera --example bench > out/hill/try/bench.txt
```

These are batch 2's comparisons: speech against the best run (it decides), the singer against the
batch's baseline (it guards). A singing batch swaps the two `--against` files.

1. **Singer.** The comparison ends with a verdict: BETTER, WORSE or NO CLEAR CHANGE, and the
   reasons when worse. `out/hill/try/report.md` has the new score table.
2. **Speech.** The speech judge ends with BETTER, WORSE or NO CLEAR CHANGE on the training
   sentences: BETTER from 1.5 points lower, WORSE past 3 points higher. It lists the sentences
   Whisper got wrong, and reports the held-out sentences, which never decide.
3. **Speed.** The bench prints the real-time load. Compare its `os=2` line with
   `out/hill/baseline/bench.txt`, the batch's baseline. It is noisy and depends on what else
   the machine is doing, so if it reads slower, rerun it and the baseline's commit back to
   back before calling a change slower.

## Keep or revert

Keep the change only if all of these hold (batch 2; a singing batch swaps 1 and 2 back, with
speech judged against the batch's baseline and required to be not WORSE):

1. The speech verdict is **BETTER**.
2. Against the batch's baseline, the singer breaks no guard and its training score rises by at
   most 0.05.
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
   - speech word error rate before → after, training and held-out
   - training and held-out singer score before → after
   - the words or terms that moved
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

- Hypothesis: what was wrong and why the change should fix it.
- Change: what changed, in which files; values tried.
- Speech: word error rate a → b (training), c → d (held-out); the words that moved.
- Singer: training score a → b, held-out c → d; the terms that moved.
- Verdict: the reasons, if reverted.
- Commit: the hash, if kept.
- Next: what this suggests trying, or not trying, next.
```

Update "Leads" at the top of the log when an experiment closes a lead or opens a new one.
