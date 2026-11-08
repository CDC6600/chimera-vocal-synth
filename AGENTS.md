# CHIMERA

A physical-modeling voice synthesizer: a Rust engine (wasm in an AudioWorklet, native for tests
and the CLI) under a React UI. See the README.

## Working here

- Do what was asked, the simplest way. Side work nobody asked for (experiments, tooling, paid
  runs) is a proposal with its cost. If the work grows well past the estimate, stop and say so.
- Before repeating or reversing earlier work, read its history (`git log -- <path>`, `git log -S`).
- Commit small. The subject names what changed and the trap, in searchable words; the body gives
  the why, what you ruled out, what you ran (with numbers) and what you didn't test.
- Keep each fixed bug's reproduction as a test.
- Code outranks this file: fix the file when they disagree. When something breaks unnoticed, add
  a rule here, with a check.

## Map

- `crates/chimera/`: the engine (no I/O). `crates/chimera-wasm/`: the browser's C ABI.
  `crates/chimera-cli/`: `chimera render | compare csd | diagnose | calibrate`.
- `src/wasm/`: the UI's facade over the engine. `src/worklet/`: the AudioWorklet. `src/ui/`: React
  and the canvas renderers.
- `tests/`: bun tests on the wasm build. `tools/`: e2e, layout lint, spectrogram, speech judge.
- `docs/`: architecture, the engine contract (`rust-engine.md`), the UI look, the singer judge,
  and the hill-climb rules.

## Rules

- **The engine's contract** (`docs/rust-engine.md`): transcendental math only through
  `chimera::math`, no allocation while rendering, output independent of block size. `cargo test`
  enforces each.
- **Any change to the sound or the plans fails the golden tests, by design.** Re-bless with
  `CHIMERA_BLESS=1 cargo test --workspace --no-fail-fast`, run twice. Human-voice audio, plans
  (including the beat sounds' gestures and the beat player), the tract geometry and the human
  airway's transfer functions re-bless; see `crates/chimera/tests/golden/README.md` for the rest.
- **`*.generated.json` tables are regenerated (`chimera calibrate`, `tools/gen-*`), never edited.**
  Vowel calibration isn't bit-reproducible, so rerun it only when the formants should move.
- **`src/wasm/chimera.wasm` is a gitignored build product.** After a Rust change, `bun test tests`
  alone tests the old engine: use `bun run test`. Don't share a cargo `target/` between checkouts.
- **Gesture records are defined twice**, in `crates/chimera-wasm/src/lib.rs` and
  `src/wasm/wire.ts`. Change both and bump `LAYOUT_VERSION`.
- **The wasm loads before the UI.** A module that calls the engine at import must not be reachable
  from `src/main.tsx`'s static imports. Check: `bun tools/e2e.ts` against `bun run preview`.
- **DOM content flows in `.screen-body`, never absolutely positioned.** Check: `bun run lint:layout`.
- **Nothing copyrighted goes in git.** `public/shows/` is gitignored; never ship CSD audio or
  anything derived from it.
- **Voice tuning follows `docs/hill-climb/README.md`** and never edits its judges.

## Checks

| When you touch | Run |
|---|---|
| anything | `bun run test` and `bun run typecheck` |
| `crates/chimera/` sound | re-bless goldens (above); `cargo run --release -q -p chimera --example bench` against the previous commit |
| `crates/chimera-wasm/`, `src/wasm/`, `src/worklet/`, `src/main.tsx` | `bun run build && bun run preview`, then `bun tools/e2e.ts` |
| `src/ui/`, `src/show/`, CSS | `bun run lint:layout` |
