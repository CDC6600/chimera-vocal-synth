# CHIMERA as a plugin

*Design, 2026-11-08. The spike (`crates/chimera-plugin`, November 2026) proved the webview works
inside FL Studio on macOS: typing, parameters both ways, two instances, reopening the window.*

The goal of the MVP: sing lyrics from a DAW's piano roll with the real React UI, as CLAP, VST3 and
AU on macOS, under the repo's MIT licence.

## What the MVP is

- **Modes:** Sing and Play. Speak, Bestiary and Beatbox stay out of the plugin for now, because
  each needs its own MIDI mapping and persisted state.
- **No demo songs and no Web MIDI.** The host gives the MIDI.
- **The sound lives in Rust.** A plugin's window is usually closed while it plays, so nothing the
  sound depends on may live in the page: lyrics, cursor, body, posture, and every note decision.
  The page is a view that sends commands.

## Formats and licence

`nih_export_clap!` plus the `clap-wrapper` crate (MIT or Apache-2.0; no CMake; it embeds the
MIT-licensed VST3 SDK and Apple's AudioUnitSDK) make one dylib export CLAP, VST3 and AUv2. A script
copies the same fat binary into `CHIMERA.clap`, `CHIMERA.vst3` and `CHIMERA.component`. nih-plug's
own VST3 export pulls in GPLv3 `vst3-sys`, so `default-features = false` turns it off.

CI validates with clap-validator, `auval` and pluginval. The workflow file only calls a script in
the repo, because the agents' GitHub App can't edit `.github/workflows/`.

## Threads

| Thread | Owns | Runs |
|---|---|---|
| Audio | `Core`: the `Engine`, the `Singer`, the telemetry scratch | `process` |
| Main (host's UI thread) | the webview, `Settings` | custom-protocol and IPC handlers, `initialize` |

They share `Shared`:

- **`Pending`, a `Mutex` the audio thread only `try_lock`s**, once per block and only if an
  `AtomicBool` says it is dirty. If the lock is busy the changes wait a block. Everything in it
  *merges* rather than queues, so a host that stops processing can't overflow it:
  `artic: ArticPatch`, `tip_free`, `source: SourcePatch`, `hold`, `mode`, `cursor`, `restart`,
  `voice: Option<Box<HumanVoice>>`, `syllables: Option<Vec<LiveSyllable>>`. Only UI notes queue, in
  a bounded array: past its size they are dropped.
- **Nothing is freed on the audio thread.** Installing a voice or syllables swaps the old one into
  a `retired` slot in the same struct, under the same lock. The main thread drops it when it next
  sends one, so the slot holds at most one of each.
- **Telemetry:** the audio thread writes its flat block into a `Mutex<Vec<f64>>` by `try_lock` and
  skips the frame if the lock is busy. The main thread holds that lock for one `memcpy`.
- **Samples for the spectrum:** a ring of `AtomicU32` (f32 bits). Torn reads only touch a picture.
- **Atomics:** cursor, sung syllable, sounding note, load, fault.

A body change builds the `HumanVoice` on the main thread and sends it through `Pending`. The audio
thread calls `Engine::install_voice`, which swaps it in and carries the performer's settings over
without allocating. `Engine::set_body` becomes `make_voice` + `install_voice`, so the browser keeps
one code path. A voice built for another sample rate is ignored.

## Real-time safety

`plan_note_on` allocates today (strings per phoneme, a `Vec` per list). The live path becomes
allocation-free without changing a single planned value:

- `plan_utterance` splits into a core over borrowed segments, using `SmallVec` for its working
  lists and a caller's `Vec<Gesture>`. It builds `PlannedSegment`s only when asked. The existing
  function stays as a wrapper, so speech, songs and the JSON API don't change.
- `LiveSyllable` is `Copy`: inline labels, at most 8 consonants on each side of the vowel (longer
  clusters are cut). `Singer` keeps these, so a note's coda survives a lyrics change.
- `Singer` moves from the plugin crate to `chimera::control::singer`. The TypeScript controller
  stays in the browser for now; moving it is a later cleanup.

Checks: the existing golden planning tests prove the plans are unchanged, and a counting-allocator
test covers `Singer` and `Core` from note-on to render (`crates/chimera/tests/beat_alloc.rs` is the
pattern).

## Determinism and idle

`process` splits blocks at each event, so note timing is sample accurate and output doesn't depend
on block size. A test compares block sizes bit for bit.

An idle voice costs as much as a singing one (29 % of a core at 44.1 kHz), which is a lot to pay for
a silent track. So `Core` stops rendering after a second of silence: an empty score, no breath, no
keys, nothing audible, and the editor not open. It decides only at fixed multiples of the sub-block
on the voice's own sample count, so the decision doesn't depend on the host's block size either. The
voice clock waits while it sleeps. Gestures are scheduled relative to that clock, so nothing shifts.

## Lyrics and the transport

Each note sings the next syllable, so the cursor has to line up with the song:

- Transport start, or a backward jump of the play position (a loop, a seek), puts the cursor back
  on the *start syllable*. It is 0 unless the user picked another in the UI while stopped.
- Transport stop releases everything sung.
- A boolean host parameter, Restart lyrics on play (default on), turns the first of these off for
  live playing.

Known limits: starting mid-song can't know how many notes came before, so the user picks the
syllable. A loop point inside a block is seen at the next block, so a note in that same block sings
the old syllable.

## State

Host parameters (automatable): output gain, vibrato depth and rate, breath, pressed, bright,
breathy, rough, fry, glide, ring, restart lyrics. IDs are permanent once released.

Everything else is one `#[persist]` JSON field, `Settings`: lyrics, body, mode, posture (the 17
articulators and `tip_free`), voiced, start syllable, and a version number. nih-plug restores it
before `initialize`, which rebuilds `Core` from it and clears `Pending`. The cursor, held notes and
the Hold breath toggle aren't saved.

Body isn't a host parameter: changing it rebuilds the airway and ends the sung notes.

## The bridge

The page comes from the same `dist/` as the website, embedded by `build.rs` (source maps left out,
the dictionary served from the bytes the plugin already embeds). `cargo test` works without a build:
the page then says the UI isn't built. `location.protocol === "chimera:"`, or a global the test
harness sets, selects plugin mode at run time. So there is no second bundle.

Page to Rust, `window.ipc.postMessage` with JSON, all handled on the main thread:

`note`, `lyrics`, `cursor`, `body`, `mode`, `artic`, `source` (for `voiced` and `f0`), `hold`,
`param { id, value: plain }`, `size`.

Parameter gestures: the bridge in the page sends `begin` before the first value and `end` 300 ms
after the last, so no control needs to know about hosts.

Rust to page, `fetch` over the custom protocol:

- `/poll`, binary, once per animation frame: fast scalars, the telemetry block (the same flat layout
  and decoder as the worklet's, moved to shared code on both sides), the active labels, and the new
  samples since the last poll. A poll also tells the audio thread for the next 250 ms that someone
  is watching. Without that it collects no telemetry and may sleep.
- `/state`, JSON, 20 times a second: parameter values with their text, mode, body and lyrics. The
  syllables are included only when their version changed.

`AudioEngine` keeps its interface: `send()` goes to the bridge instead of the worklet port, and
`analyser` is a small class with `AnalyserNode`'s Blackman window, smoothing and decibels, doing its
FFT in JS. The page's wasm stays: the anatomy model, geometry and vowel map still use it. In plugin
mode, `store.ts` and `singing.ts` send notes and lyrics to Rust and take the cursor, the sung
syllable and the piano roll's notes from the polls. When a poll brings a host value that differs
from the store's and no gesture is active, the store updates without sending it back.

Keys held in the UI are released when the window closes.

## Testing without a Mac or a DAW

- `Core` has no nih-plug types, so its tests run anywhere: MIDI to audio, allocation, block-size
  independence, sleep, state restore, voice swap, transport.
- `examples/ui_harness.rs` serves the same `/poll`, `/state` and IPC over HTTP from a real `Core` on
  a timer. A Playwright script drives the UI against it in headless Chromium, and the layout lint
  checks the plugin's window size.
- CI on macOS builds all three bundles and runs the validators. Only the owner's FL Studio tests the
  webview itself.

## Later

- **Lookahead.** A vowel should land on the beat with its consonants ahead of it. That needs
  reported latency and delayed events, and live playing wouldn't want it, so it would be a toggle.
- Windows (WebView2 through wry). Most FL Studio users are on Windows.
- Speak, Bestiary and Beatbox. Host tempo sync for Beatbox.
- A window size selector (nih-plug can't be resized by the host, only ask to resize itself).
- Signing, notarisation and licence notices for a public release.
- One `Singer` for the browser too, replacing the TypeScript controller.
