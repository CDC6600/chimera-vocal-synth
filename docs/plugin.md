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

CI validates with clap-validator, `auval` and pluginval. The workflow file only calls scripts in
`crates/chimera-plugin/macos`, because the agents' GitHub App can't edit `.github/workflows/`.

## Threads

| Thread | Owns | Runs |
|---|---|---|
| Audio | `Core`: the `Engine`, the `Singer`, the telemetry scratch | `process`, `reset` |
| Main (host's UI thread) | the webview, `Settings` | custom-protocol and IPC handlers |
| Whichever thread the host restores or activates on | `Core` while replacing it | `initialize` |

nih-plug takes its plugin lock around `process` and `initialize`, so they never overlap. A state
restore that arrives while playing blocks the audio thread for the length of `initialize`, which
glitches, as loading a preset usually does. **The plugin must never call `GuiContext::set_state`:**
nih-plug runs that restore on the audio thread.

They share `Shared`:

- **`Pending`, a `Mutex` the audio thread only `try_lock`s**, once per block and only if an
  `AtomicBool` says it is dirty. If the lock is busy the changes wait a block. Everything in it
  *merges* rather than queues, so a host that stops processing can't overflow it:
  `artic: ArticPatch`, `tip_free`, `source: SourcePatch`, `hold`, `mode`, `cursor`,
  `voice: Option<(Box<HumanVoice>, BodyMix)>`, `syllables: Option<Vec<LiveSyllable>>`. `initialize`
  clears it, and records the sample rate there, so a voice built for another rate is dropped when
  the main thread tries to publish it, never on the audio side.
- **UI notes** are the exception that must queue, in a bounded array. On overflow the array empties
  and a `clear_ui_keys` flag is set instead: Core then releases every key the UI holds, so a
  dropped release can't leave a stuck note. Closing the window sets the same flag.
- **Nothing is freed on the audio thread.** Installing a voice or syllables swaps the old one into
  a `retired` slot in the same struct, under the same lock. The main thread empties that slot under
  the lock before it publishes another, so it holds at most one of each.
- **Telemetry:** the audio thread swaps its flat block with one in a `Mutex`, by `try_lock`, and
  skips the frame if the lock is busy. Both are sized for the largest body at this sample rate, so
  a body change can't make the next snapshot allocate.
- **Atomics:** cursor, sung syllable, sounding note, load, fault, playing, and a state revision.

A body change builds the `HumanVoice` on the main thread and sends it through `Pending`. The audio
thread calls `Engine::install_voice`, which swaps it in and carries the performer's settings over
without allocating. `Engine::set_body` becomes `prepare_voice` + `install_voice`, so the browser
keeps one code path.

**Who holds a note.** A pitch can be held by the host's MIDI and by the on-screen keys at once. Core
keeps one mask for each and tells the `Singer` only about the first press and the last release, as
`src/ui/midi.ts` does across keyboards.

## Real-time safety

`plan_note_on` allocates today (strings per phoneme, a `Vec` per list). The live path becomes
allocation-free without changing a single planned value:

- `plan_utterance` splits at the point its durations are known. `plan_timed` takes borrowed
  segments, uses `SmallVec` for its working lists and a caller's `Vec<Gesture>`, and builds
  `PlannedSegment`s only when asked. Live notes give every segment a duration, so they skip
  `klatt_durations`, which allocates.
- `LiveSyllable` is `Copy`: inline labels, at most 8 consonants on each side of the vowel. Longer
  clusters are cut, in the browser too; no English word has one. `Singer` keeps these, so a note's
  coda survives a lyrics change.
- `Singer` moves from the plugin crate to `chimera::control::singer`. The TypeScript controller
  stays in the browser for now; moving it is a later cleanup.

Checks: the golden planning tests prove the plans are unchanged, and counting-allocator tests cover
`Singer` and `Core` from note-on to render, including the longest syllable and a body swap.

## Determinism

`process` splits blocks at each event, so note timing is sample accurate and output doesn't depend
on block size. A test compares block sizes bit for bit, from a fresh `Core`. Taking telemetry doesn't
change the sound either, so whether the window is open makes no difference; a test checks that too.

- **Flush-to-zero.** nih-plug turns it on around `process`, which `docs/rust-engine.md` forbids for
  the sake of identity with the browser. A plugin has nothing to be identical to, so the plugin
  accepts it: a test renders with it on and checks the voice stays stable and close to the same.
- **`reset` is not a full DSP reset.** It stops what is sung, clears the score, and puts the cursor
  back. Glottal phase, the noise generators and the voice clock carry on, so a second render after
  a bare `reset` matches the first to the ear, not to the bit. A host that re-activates the plugin
  (`initialize`) gets a fresh voice and an exact match. Clearing every DSP state in place, without
  allocating, is a larger engine change, deferred until someone hears a difference.

## Lyrics and the transport

Each note sings the next syllable, so the cursor has to line up with the song:

- Transport start, or a backward jump of the play position (a loop, a seek), puts the cursor back
  on the *start syllable*. It is 0 unless the user picked another in the UI while stopped. The
  position is read in beats where the host gives them, because a tempo change makes a position
  computed in samples jump.
- Transport stop releases everything sung.
- A boolean host parameter, Restart lyrics on play (default on), turns the first of these off for
  live playing.

Known limits: starting mid-song can't know how many notes came before, so the user picks the
syllable. A loop point inside a block is seen at the next block, so a note in that same block sings
the old syllable.

## MIDI

Supported: notes, velocity (breath), pitch bend (±2 semitones), and all-notes-off / all-sound-off.
Other controllers are ignored for now. Through clap-wrapper's VST3 layer a controller keeps only its
last value per block, so bend moves in block-sized steps there; note timing isn't affected.

## State

Host parameters (automatable): output gain, vibrato depth and rate, breath, pressed, bright,
breathy, rough, fry, glide, ring, restart lyrics. IDs are permanent once released.

Everything else is one `#[persist]` JSON field, `Settings`: lyrics, body, mode, posture (the
articulators changed from neutral, and `tip_free`), voiced, the Play-mode pitch, the start syllable,
and a version number. nih-plug restores it before `initialize`, which rebuilds `Core` from it, clears
`Pending` and bumps the state revision. The cursor, held notes and the Hold breath toggle aren't
saved.

Body isn't a host parameter: changing it rebuilds the airway and ends the sung notes.

## The bridge

The page comes from the same `dist/` as the website, embedded by `build.rs` (source maps and the
dictionary left out: Rust does the lyrics). `cargo test` works without a build: the page then says
the UI isn't built. `window.ipc`, which wry defines and the harness imitates, selects plugin mode at
run time, so there is no second bundle.

Page to Rust, `window.ipc.postMessage` with JSON, handled on the main thread:

`note`, `lyrics`, `cursor`, `body`, `mode`, `artic`, `source` (for `voiced` and `f0`), `hold`,
`param { id, value: plain }`.

Parameter gestures: the page's bridge sends `begin` before the first value and `end` 300 ms after
the last, so no control needs to know about hosts. Rust remembers which gestures are open and ends
them when the window closes. The page's timer dies with it, and clap-wrapper's VST3 layer ignores
automation on a parameter whose gesture never ended.

Rust to page, `fetch` over the custom protocol:

- `/poll`, binary, once per animation frame: the fast scalars, the telemetry block (the same flat
  layout and decoder as the worklet's, moved to shared code on both sides) and the active labels. A
  poll also tells the audio thread for the next 250 ms that someone is watching; without that it
  collects no telemetry.
- `/state`, JSON: the whole snapshot of `Settings`, parameters and syllables.

**One owner for each value.** `mount()` awaits a `/state` snapshot and fills the stores before the
first render, and in plugin mode `start()` sends nothing: replaying browser defaults would overwrite
a restored project. After that:

- `Settings` values are written only by the page. It reads them again only when the revision in
  `/poll` changes, which means a restore happened.
- Parameters can also change from the host, so the page polls them 20 times a second and skips any
  with a gesture open.
- The cursor, sung syllable and sounding note are read from every poll.

`AudioEngine` keeps its interface: `send()` goes to the bridge instead of the worklet port. In plugin
mode `store.ts` and `singing.ts` send notes and lyrics to Rust and take the syllables, the cursor and
the piano roll's notes from it. The page's wasm stays: the anatomy model, geometry and vowel map
still use it. The spectrum shows the airway's transfer function without the live harmonics until the
analyser arrives.

## Testing without a Mac or a DAW

- `Core` has no nih-plug types, so its tests run anywhere: MIDI to audio, allocation, block-size
  independence, flush-to-zero, state restore, voice swap, note ownership, transport.
- `examples/ui_harness.rs` serves the same `/poll`, `/state` and IPC over HTTP from a real `Core` on
  a timer. A Playwright script drives the UI against it in headless Chromium, and the layout lint
  checks the plugin's window size.
- CI on macOS builds all three bundles and runs the validators. Only the owner's FL Studio tests the
  webview itself, so a build reaches it as early as the full bundle can load: whether module scripts
  and wasm work over the custom scheme in WKWebView is the largest unknown left.

## Before anything is shared publicly

Licence notices for every dependency in the binaries, in the download. Until then builds are for
testing only.

## Later

- **Idle sleep.** An idle voice costs as much as a singing one (29 % of a core at 44.1 kHz). Sleeping
  is cheap to write, but a sleeping voice freezes glottal phase, vibrato and noise, so the decision
  must not depend on the window being open or on wall-clock time, and the display needs a clock that
  keeps moving. FL Studio's Smart Disable covers much of this meanwhile.
- **The spectrum's live harmonics:** a sample ring from the audio thread, and a small class with
  `AnalyserNode`'s window, smoothing and decibels doing its FFT in JS.
- **Lookahead.** A vowel should land on the beat with its consonants ahead of it. That needs
  reported latency and delayed events, and live playing wouldn't want it, so it would be a toggle.
- Windows (WebView2 through wry). Most FL Studio users are on Windows.
- Speak, Bestiary and Beatbox. Host tempo sync for Beatbox.
- Sustain pedal, mod wheel and other controllers.
- A window size selector (nih-plug can't be resized by the host, only ask to resize itself).
- A full DSP reset (see Determinism).
- Signing and notarisation.
- One `Singer` for the browser too, replacing the TypeScript controller.
