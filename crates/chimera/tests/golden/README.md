# Golden fixtures

These are what the TypeScript engine computed before it was retired. The generators lived in
`tools/golden/*.ts`; recover them with `git show fdf5441:tools/golden/<area>.ts`. They ran the
TypeScript engine that is still in that commit's `src/engine/`. The `golden_*` tests (and
`engine.rs`) check that the Rust engine reproduces the fixtures. This is how the port was
verified: numbers to 1e-9 relative, and audio sample for sample.

| Fixture | What | Test |
|---|---|---|
| `geometry.json` | grids, area functions, contacts and outlines for 114 postures | `golden_geometry.rs` |
| `voice.json`, `voice-*.f32` | human-voice scenarios (timed operations) and their samples | `golden_voice.rs` |
| `planning.json`, `beatbox-bar.f32` | plans, durations, sung notes, beatbox builds, sequencer, one bar | `golden_planning.rs`, `engine.rs` |
| `phonetics-*.json` | IPA tokenizer, G2P, phrases, syllables, songs and vowel data | `golden_phonetics_text.rs` |
| `creatures.json`, `creatures.f32`, `creature-units.json` | every creature's calls and controls, valves | `golden_creatures.rs` |
| `analysis.json` | transfer functions, FFT, LPC, YIN, Nelder–Mead | `golden_analysis.rs` |

When a test fails after an **intended** change to the sound, the plans or the tract:
- **These regenerate from the Rust engine** with `CHIMERA_BLESS=1 cargo test --workspace
  --no-fail-fast`, run twice (`engine.rs` reads a fixture `golden_planning` writes): the
  human-voice audio (`voice-*.f32`, `beatbox-bar.f32`), the plans in `planning.json` (utterances,
  durations, phrases, songs, live notes, beat sounds, the beat player), `geometry.json`, and the
  human airway's cases in `analysis.json`. They are snapshots of this engine now.
- **The rest** still records the TypeScript engine's behaviour: the beat notation, the phonetics
  text, the creatures, and the analysis code's own cases (networks, FFT, LPC, YIN, Nelder–Mead).
  Either update the expectation the change affects, or remove the case and add a behavioural test
  (`tests/*.rs`) for what the change is meant to do.
