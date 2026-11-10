# UI redesign: the vector display (issue #2)

> Note: the design language (vector display) is settled; the look within it can still evolve. See README → Vision & scope.

The approved mockups are in `docs/mockups/` (open `index.html`). This plan says how to build them into the app on top of the Rust engine. The look is settled; this is about how to get there without breaking the instrument.

## What the mockups fix

- **Look.** WarGames NORAD screens: thin glowing lines on pure black, no filled panels, crisp glow (not bloom-heavy), no scanlines or curvature, no terminal-style typing.
- **Colour roles.** Blue is the body and structure. Amber is sound (pressure, flow, spectra). Green, red, pink and white are four control slots: each screen maps its own four controls onto them, OP-1 style, and draws what they move in the same colour.
- **Type.** Hershey Simplex single-stroke capitals everywhere. IPA glyphs Hershey lacks are drawn as strokes.
- **Layout.** A wall of screens: a tall left screen (the face, or the creature) and a right column of instrument screens that changes per mode.
- **Anatomy.** Outlines from the engine's geometry; the airway's cross-section lines lit by pressure; airflow streaks; turbulence dots; fading afterglow of the tongue, jaw and lips; four big slot readouts under the chin.

## Rendering

**Canvas 2D, with one cheap shared glow.** Every renderer today is Canvas 2D (`useCanvasLoop`, `AnatomyRenderer`), so the drawings stay Canvas 2D and the glow becomes one shared step:

1. Draw sharp lines and text with `globalCompositeOperation = "lighter"`, so crossings add up like a beam.
2. Copy the canvas down to one reduced-resolution layer, blur it there, and add it back on top with `"lighter"`. The blur runs on a fraction of the pixels; the full-resolution composite is the remaining cost, and it gets measured.
3. Screens keep drawing every frame, as today. Telemetry, afterglow, scrolling views and `tickSinging()` all change outside the store, so skipping frames needs an invalidation contract first, and only if measurement asks for it.

A hook, `usePhosphor(draw)`, replaces `useCanvasLoop` for the vector screens: DPR, resize, the rAF loop, the glow pass, and a small kit (`beam(path, colour, width)`, `label(text, …)` in the Hershey font). One convention everywhere: the context is pre-scaled so drawing and hit-testing are in CSS pixels, with DPR capped at 2. The anatomy renderer moves to it from its backing-pixel coordinates.

**DOM for chrome.** Screen frames, titles, chips, tabs, text fields and encoders are DOM, in the Hershey font with CSS glow (`text-shadow`, `box-shadow`). They stay accessible and need no canvas work.

**WebGL only if a trace says so.** The renderers use paths, text, gradients and image data, so a WebGL move would change them too. It waits until a trace shows Canvas 2D is the bottleneck.

**Budget.** Under 8 ms of main-thread time per frame for the Play screen at 2× DPR, attributed with a Chromium trace on this machine, recording canvas sizes, DPR and the graphics backend. The Play screen is benchmarked as soon as it exists, before the other modes are ported.

## Layout

- `App` becomes a top bar plus a grid: the tall left screen and a right column. Each mode defines its right column (as in the mockups).
- Below about 1100 px wide, the screens stack in one column: the left screen first, then the rest in reading order. Phones get the same stack.
- A `Screen` component draws the frame, title and subtitle; `focus` brightens the frame of the screen the encoders are bound to. Its content area always has a positive size (a minimum height per screen), and canvases fill it absolutely, so nothing collapses when stacked.
- Wide content (the consonant chart, the sequencer) scrolls horizontally inside its screen on narrow layouts.

## Controls

- **Encoders replace sliders.** `Encoder` draws a dial with a big numeral in its slot colour over a native `<input type="range">` that does the work: a visible label, keyboard (arrows, Page Up/Down, Home/End), `aria-valuetext`, pointer capture and cancellation. Logarithmic values (pitch, glide) keep the current mapping. `touch-action: pan-y` keeps vertical swipes scrolling the page, and the wheel doesn't change values. Values stay in the existing stores; the page shown is presentation only. MIDI CC (the app's MIDI handles notes only today) comes later.
- **Pages of four.** An encoder screen shows four and pages through the rest:
  - Play: VOICE (pitch, breath, pressed, vibrato), TONE (breathy, rough, fry, glide), MOUTH (lip round, lip open, protrude, jaw), THROAT (tongue root, tip curl, velum, larynx), LARYNX (twang, false folds).
  - Sing: vibrato depth, rate, breathy, ring. Beatbox: tempo, swing, mic, bass note. Bestiary: the creature's params, four per page.
- **Chips** replace toggles and segmented buttons: a framed caps label, lit when on. Mode and body tabs are chips in the top bar.
- **View options** (pressure, airflow, measurements, grid) become chips in the face screen's corner.
- **Start prompt** becomes a framed overlay on the left screen with one lit chip ("BREATHE IN AND START").

## Screens per mode

| Mode | Left | Right column |
|---|---|---|
| Play | face (`AnatomyView`) | tube, vocal folds, spectrum, vowels, voice encoders, keys |
| Speak | face | say (text field, chips, transcription), gestural score, consonant chart, tube, vocal folds |
| Sing | face | piano roll, lyrics chips, spectrum with the ring zone, song encoders, keys |
| Beatbox | face with the hit's big symbol | step sequencer, waterfall spectrogram, pads, transport encoders + presets, compact keys |
| Bestiary | creature (`CreatureView`) | creature gallery, waterfall spectrogram, calls, creature encoders, unrolled airway, compact keys |

`Keys` also owns the computer-keyboard listeners and the MIDI connect button, so every mode that has keys today keeps at least a compact keys strip. (Speak never had them: it plays text, not notes.)

The mockup generator (`docs/mockups/gen/`) is the reference implementation for each drawing: the same geometry, colours and layout rules, written against the live data the app already has (`HumanFrame`, telemetry, transfer functions, plans, creature visuals).

## Phases

Each phase is a commit on `t3/ui-mockup-inspiration`, pushed, with the app working at every step.

1. **Foundations.** Hershey fonts in `public/fonts`, colour tokens (CSS and `colors.ts`), `Screen`, `Chip` (keeping button, radio and toggle semantics), `Encoder`, `usePhosphor`, the app shell and top bar, the start overlay. Restyle everything else coarsely (black, frames, Hershey) so no screen looks broken.
2. **Play.** The face renderer in outlines (afterglow, turbulence, slots), tube, vocal folds, spectrum, vowel chart, encoders, keys. Then the benchmark, at 2× DPR and at phone width.
3. **Speak, Sing, Beatbox, Bestiary, one at a time.** They share renderers (anatomy, spectrum variants, the waterfall, keys), so those settle first and each mode then owns only its composition and scoped styles.
4. **PR.** Each phase already checks the narrow layout, keyboard use and performance; this one is the final pass and the PR.

## Testing

- `bun run typecheck`, `bun test tests`, `cargo test --workspace` (the engine doesn't change).
- `tools/e2e.ts`: loads every mode, plays, saves screenshots. It now exits non-zero when it collected errors, keeps its audio assertions, and adds checks for encoder changes, touch cancellation, resizing, and switching modes while playing. Fresh start-up in each mode, including the retry path.
- Chrome text keeps its natural case in the DOM (capitals come from CSS), so accessible names and the e2e selectors don't change.
- Compare each mode's screenshot with its mockup at the same viewport and a similar state; the waveforms won't match the old TS engine's exactly.
- The frame budget comes from a Chromium trace, not rAF deltas.

## Risks and open questions

- **Software rendering.** Headless Chromium has no GPU, so e2e frame times will be pessimistic; the budget is for the real machine.
- **IPA.** Text stays Unicode everywhere (inputs, chart buttons, transcriptions), in the font stack `Hershey, Archivo`: Archivo already covers IPA Extensions. Stroke glyphs (from `docs/mockups/gen/`) are only for decorative canvas symbols, with text equivalents. Canvases redraw when fonts finish loading.
- **Start-up order.** `main.tsx` loads the wasm module before any UI module, because the engine facade reads its catalogue at import. Shared helpers must not import the facade earlier.
- **Encoders on touch.** A small dial is fiddly on a phone; the stacked layout gives each encoder a wider cell.
