# CHIMERA as a plugin

*Design, 2026-11-08. The spike (`crates/chimera-plugin`, November 2026) proved the webview works
inside FL Studio on macOS: typing, parameters both ways, two instances, reopening the window.*

The goal of the MVP: sing lyrics from a DAW's piano roll with the real React UI, as CLAP, VST3 and
AU on macOS, under the repo's MIT licence.

## What the MVP is

- **Modes:** Sing, Play, Speak and Beatbox. Bestiary stays out for now: it needs its own MIDI mapping
  and persisted state.
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
`src/ui/midi.ts` does across keyboards. A repeated note-on from the same owner is still a new note, so
overlapping notes in a piano roll keep the lyrics in step.

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

**A restore is a boundary.** Every command carries the revision the page last adopted, and Rust drops
one from before a restore (except the end of a gesture, which must still close). The page sends edits
at once instead of coalescing them per frame, so none can wait out a restore. A short transaction lock,
never on the audio thread, keeps a command's settings and its `Pending` change together against
`initialize`.

**One owner for each value.** `mount()` awaits a `/state` snapshot and fills the stores before the
first render, and in plugin mode `start()` sends nothing: replaying browser defaults would overwrite
a restored project. After that:

- `Settings` values are written only by the page. It reads them again only when the revision in
  `/poll` changes, which means a restore happened.
- Parameters can also change from the host, so the page polls them 20 times a second and skips any
  with a gesture open.
- The cursor, sung syllable, sounding note and a press count are read from every poll. The count tells
  a new note on the same pitch from one held. The piano roll and the words at the mouth follow these,
  and clear when the voice clock starts again.

`AudioEngine` keeps its interface: `send()` goes to the bridge instead of the worklet port. In plugin
mode `store.ts` and `singing.ts` send notes and lyrics to Rust and take the syllables, the cursor and
the piano roll's notes from it. The page's wasm stays: the anatomy model, geometry and vowel map
still use it.

## Window size

The window opens at the size the project saved (`#[persist = "window"]`, 1280 x 800 at first) and can
be anything from 720 x 500 to 3840 x 2160.

- **Larger than the layout needs:** the screens take the room, as on the website.
- **Smaller than `LAYOUT_MIN` (1120 x 800):** the page is zoomed out (`WKWebView.pageZoom`) until it
  fits, so the layout stays the same, smaller. The website's own answer to a narrow window is a stacked
  page that scrolls, and in a plugin what's below the fold might as well not be there. The layout lint's
  `--plugin` mode fails if the page scrolls at `LAYOUT_MIN`.

nih-plug answers `can_resize` with false, so a host can't drag the border. Patching that means forking
nih-plug, so the window has its own grip in the corner, as many plugins do. While dragging, the page
tells Rust the pointer moved, at most once a frame, and Rust reads where to (`NSEvent.mouseLocation`):
the page's own coordinates change with its zoom, which changes during the drag. Double-clicking the grip
goes back to the first size.

- **`request_resize`'s answer can't be trusted.** clap-wrapper's VST3 layer tests `!resizeView(...)`, and
  `kResultOk` is 0, so success returns false and failure true. Its AU layer sets the frame and always
  returns false. So Rust ignores the answer, and half a second after the grip is let go it takes the
  host view's real size as the window's: a host may have refused or clamped it, and a CLAP host may
  resize a little after it's asked.
- **A host may refuse.** clap-wrapper's `onSize` reports failure while `can_resize` is false, and a host
  that honours that keeps its old size. Only a test in each host can tell; if FL Studio does, the way
  out is `can_resize` in a nih-plug fork, which would also give dragging the border.
- **The webview follows the host's view** through its autoresizing mask (wry fixes a child's size, so
  the editor changes the mask after building it). Nothing calls `set_bounds`: a host that resizes
  after the call returns would grow the webview twice. The zoom follows the view's real size too, read
  at every poll.

## Speak

Each note says the next line, as a note in Sing sings the next syllable. A line is what's between
line breaks, and punctuation inside one makes phrases with pauses, as on the website.

- **Lines are planned on the main thread**, when the text, the IPA toggle, the rate or the body
  changes, and at `initialize`: `to_phrases` (or the IPA tokenizer) then `plan_phrases`, for
  `default_f0(body)`. The audio thread gets a `Vec<Vec<Gesture>>` through `Pending`, and hands the old
  one back through a `retired` slot. A note only calls `Engine::schedule`, which copies into reserved
  buffers, so it allocates nothing. A line is cut at `MAX_SCHEDULE` (4000) gestures, about a minute.
- **The key transposes, like a sampler.** C4 (60) speaks at the body's natural pitch; each semitone
  moves the whole contour through `HumanVoice::bend`, on top of the pitch wheel. A plan isn't rebuilt
  per note. Velocity does nothing here.
- **A note is a one-shot.** Its release does nothing. A new note cuts the line sounding (`replace`).
- Transport start or a loop goes back to the *start line* if Restart Lyrics On Play is on, and a stop
  stops the speech. Picking a line while stopped sets the start line.
- The Speak button says the next line at natural pitch. The consonant chart's pads plan one `ˈɑpɑ` on
  the main thread and send it as a one-shot through `Pending` (latest wins).

The page takes what's being said from Rust: `/poll` carries a spoken serial, the line and the voice
clock of its time 0; when the serial changes the page fetches `/speech/<line>` (the phrases and the
plan), and the score, the sound readout and the words at the mouth work as they do on the website.
Every set of plans has a generation, published with the serial and returned with the plan. Audio can
be sounding from plans already replaced (the text changed during a line), so a reply from another
generation is dropped rather than drawn over the wrong sound. The pads' one-shot has its own generation
and its own route.

## The live spectrum

The audio thread copies each block's output into a bounded sample ring in `Shared`, by `try_lock` and
only while someone is watching; a busy lock loses a block of display, no more. `/poll` drains it. A
small class on the page has the part of `AnalyserNode` that `SpectrumView` and `Spectrogram` use
(Blackman window, FFT, smoothing, decibels), so those views don't change.

## Beatbox

**Keeping time.** Every mouth sound needs 100 to 174 ms of set-up before its hit (measured: kick 137,
snare 130, hat 106, click 174, for every body), so a hit started when its note arrives lands late and
unevenly. In Beatbox mode the plugin reports `BEAT_LATENCY` = 180 ms and puts each hit exactly that far
after the point on the host's timeline that asks for it; the host's delay compensation does the rest.
This makes the hard cases simple:

- The first hit after pressing play has its full set-up, and lands on the grid.
- A step is scheduled when the transport reaches it, not predicted ahead, so a loop or a seek leaves
  nothing to cancel: hits in flight belong to the time before the jump.
- Each step's time comes from the host's position in that block, so a tempo change needs no anchor.

**Changing mode.** The latency is 0 in every other mode, because live playing can't take it. A new mode
and its timing take effect on the audio thread at once, through `Pending`, and don't depend on
`initialize` running again. What the host then does depends on the format, because nih-plug calls
`request_restart` while processing and `latency.changed` otherwise:

- **CLAP:** the host restarts the plugin, so there's a short gap and `initialize` rebuilds `Core`.
- **VST3:** clap-wrapper turns it into `restartComponent(kLatencyChanged)`, and the host reads the new
  latency. It may or may not reactivate.
- **AU (known limit):** clap-wrapper ignores `request_restart`, so while processing the host isn't told.
  Compensation stays as it was, and hits are 180 ms off in one direction or the other until the host
  next activates the plugin (reloading the project does it). That is a lasting timing error, not a gap,
  and fixing it means patching clap-wrapper.
- A host with no delay compensation hears everything in Beatbox 180 ms late.

**The sequencer** follows the host while it is playing and gives a position and tempo (Follow Host in
the transport panel, on by default, saved). The pattern's step number comes from the host's position,
so it stays in phase with the bars, and swing delays the odd steps as before. `BeatPlayer` gets a
second way in beside the voice-clock `tick`: `follow`, which schedules the steps reached in this block.
While the host is stopped, the Play Beat button runs the pattern on the voice clock at its own tempo,
as on the website; a host starting takes over from it.

`follow` keeps the next step's number and asks only where each block starts:

- Steps fire when the position reaches them, and a step's number only moves forward, so a host that
  repeats a position for a block, or reports a slightly uneven one, can't double a hit. A step reached
  late still gets its true time, since `BEAT_LATENCY` leaves room.
- Only a jump of more than two blocks (or a quarter of a step, whichever is more) from where the last
  block ended looks the step up again.
- While the position is negative (a host's pre-roll) nothing fires; step 0 waits for beat 0.
- **Known limit:** nih-plug makes up beats from seconds and the current tempo for a host that gives no
  beats, and doesn't say so. In such a host a tempo change moves the position, and the pattern jumps
  phase. Hosts that give beats, FL Studio among them, aren't affected.

The voice clock moves in control steps of 64 samples, so a hit's time is good to within one (1.5 ms at
44.1 kHz). At block sizes that are multiples of 64 the output doesn't depend on block size, and it matches
the website's sequencer to the sample for the same delay. A kick's peak varies with exactly when it falls
(0.11 to 0.25 across delays, on the website's sequencer too), so a pattern's levels aren't all equal.

**Layout.** At the plugin's window sizes the pads spread across the width and the waterfall sits beside
the transport, and each pad's airstream moves to its tooltip, so the whole of Beatbox fits without
scrolling. The website's layout doesn't change.

**Notes.** Host notes below C4 (60) are pads, with General MIDI's drum kit mapped onto the ten mouth
sounds, so a drum clip plays straight away, as beatbox rather than as its original kit: 35, 36 kick; 37
rimshot; 38 snare; 39 click; 40 inward snare; 41, 43, 45, 47, 48, 50 lip pop; 42, 44, and the ride's 51,
53, 59 hi-hat, since they keep time; 46 open hat; 49, 52, 55, 57 crash. Others below 60 are ignored. C4 and up hum, two octaves down, so a bass
line sits on the keyboard's middle. Velocity sets the accent. The on-screen keys hum at the pitch they
show, and the pads have their own buttons.

Because the same MIDI number means a different pitch from each owner here, the shared press and release
masks don't apply in Beatbox: held hums are kept per owner, as sounding pitches.

While the host is playing, a note's hit goes `BEAT_LATENCY` after the note, on the grid. While it's
stopped, it goes as soon as the mouth can make it (the website's `beat_hit`), because someone is
auditioning. Which of these applies comes from the block's own transport, so a pad queued just before
the host starts is already on its timeline. The hum is the same sound the sequencer uses, built open-ended
with a tag; its release is `GestureScore::release` at the matching time, and it's monophonic, falling back
to the newest key held. Stopping the sequencer clears the score, so a hum still held starts again.

**Close mic.** The website's chain is Web Audio nodes (low shelf +9 dB at 120 Hz, peak +3 dB at 3.5 kHz,
compressor at -28 dB, 5:1, make-up), which a plugin doesn't have, and much of a beatbox's weight comes
from it. `CloseMic` is the same chain in Rust on the audio thread, per sample and allocation-free, used
only in Beatbox with Close Mic on (saved, on by default). It isn't bit-identical to the browser's.

**State.** `Settings` gains `speech { text, ipa, rate, startLine }` and `beat { steps, bpm, swing,
preset, mic, follow }`, each with defaults, so older projects load. `playing` isn't saved. Entering
Beatbox sets the ready posture as the website does. Rust does that as part of the mode command, under
one transaction and in one `Pending` edit, so neither a held note nor a restore can separate them.

## Updating

`crates/chimera-plugin/macos/update.sh` installs CI's newest passing build into
`~/Library/Audio/Plug-Ins`. It needs a logged-in `gh`, because GitHub wants an account to download a run's
artifacts. It removes each old bundle rather than overwriting it, because changing the file of a binary a
host has loaded would crash the host. A running host keeps the old version until it restarts.

The plugin doesn't replace itself, though it could on a Mac:

- It would be a way to run new code on every install, so it would need its own signature check, and a key
  that can't leak: anyone who got past that would reach every user's machine.
- It still takes a host restart, Windows locks a loaded DLL, and a sandboxed host may not let it write.
- Musicians don't expect an instrument in their session to go online and change itself.

For a public release, the plan is a notice in the plugin that a new version exists (off with one click, and
making no other network use) and a signed installer: see Later.

## Testing without a Mac or a DAW

- `Core` has no nih-plug types, so its tests run anywhere: MIDI to audio, allocation, block-size
  independence, flush-to-zero, state restore, voice swap, note ownership, transport, spoken lines,
  beat steps against a simulated host transport (loops, seeks, tempo changes), the drum map, close mic.
- `examples/ui_harness.rs` serves the same `/poll`, `/state` and IPC over HTTP from a real `Core` on
  a timer. A Playwright script drives the UI against it in headless Chromium, and the layout lint
  checks the plugin's window sizes.
- CI on macOS builds all three bundles and runs the validators. Only the owner's FL Studio tests the
  webview itself, so a build reaches it as early as the full bundle can load: whether module scripts
  and wasm work over the custom scheme in WKWebView is the largest unknown left.

## Licences

Every bundle carries `LICENSE` and `THIRD-PARTY-LICENSES.txt` in `Contents/Resources`, generated at
build time by `tools/third-party-notices.ts` from what the build really uses: the plugin crate's
dependencies, what `clap-wrapper` compiles, the page's JavaScript and the dictionary. The script fails
if a dependency has no licence, or only a GPL one.

## Later

- **Idle sleep.** An idle voice costs as much as a singing one (29 % of a core at 44.1 kHz). Sleeping
  is cheap to write, but a sleeping voice freezes glottal phase, vibrato and noise, so the decision
  must not depend on the window being open or on wall-clock time, and the display needs a clock that
  keeps moving. FL Studio's Smart Disable covers much of this meanwhile.
- **Lookahead in Sing.** A vowel should land on the beat with its consonants ahead of it. Beatbox's
  reported latency shows how; live playing wouldn't want it, so in Sing it would be a toggle.
- Windows (WebView2 through wry). Most FL Studio users are on Windows.
- Bestiary.
- Sustain pedal, mod wheel and other controllers.
- Dragging the host's window border (needs `can_resize` in nih-plug: a fork or an upstream change).
- A full DSP reset (see Determinism).
- Signing, notarisation and an installer, with a notice in the plugin when a new version exists.
- One `Singer` for the browser too, replacing the TypeScript controller.
