/**
 * Sing mode's live path end to end, without a browser: the key handling in src/ui/singing.ts
 * talks to a stand-in for the audio engine that does what the worklet does with each message
 * (schedule at clock + lead, release by tag, cancel), on the real engine (the wasm build, in its
 * own instance as in the worklet).
 */
import { describe, expect, mock, test } from "bun:test";
import { initEngine, WasmVoice } from "./harness.ts";
import type { Gesture } from "../src/wasm/types.ts";
const MALE = { female: 0, child: 0 };

await initEngine();
const { activation } = await import("../src/wasm/engine.ts");
const midiToHz = (m: number) => 440 * Math.pow(2, (m - 69) / 12);

const FS = 48000;
let voice = new WasmVoice(FS);
/** what the worklet answers when a scheduled plan starts (singing.ts registers one at import) */
let planned: (m: { type: "planned"; id: number; t0: number; end: number }) => void = () => {};
const fakeEngine = {
  latest: null,
  voiceClock: () => voice.clock,
  onPlanned: (f: typeof planned) => ((planned = f), () => {}),
  scheduleGestures: (gs: Gesture[], lead = 0.006) => {
    voice.schedule(gs, lead);
  },
  schedule: (plan: { gestures: Gesture[] }) => {
    voice.schedule(plan.gestures, 0.03, true);
    return 1;
  },
  release: (tag: number) => voice.release(tag),
  stopSpeech: () => voice.stop(),
};
let startAudio: () => void = () => {};
const engineStub = new Proxy(
  {
    ...fakeEngine,
    start: () => new Promise<void>((r) => (startAudio = r)),
  } as Record<string, unknown>,
  // every other engine call the store makes (volume, body, gate…) does nothing here
  { get: (t, k) => (k in t ? t[k as string] : () => {}), set: (t, k, v) => ((t[k as string] = v), true) },
);
mock.module("../src/audio/AudioEngine.ts", () => ({ engine: engineStub }));
const { useSing, singNoteOn, singNoteOff, playDemo, stopSinging, tickSinging } = await import("../src/ui/singing.ts");
const gesturesOf = () => voice.score();
/** breath the score is giving now (strength-weighted activation of breath gestures) */
const breathNow = () => {
  const t = voice.clock;
  let breath = 0;
  for (const g of voice.score()) if (g.targets.breath !== undefined) breath += activation(g, t) * g.strength;
  return breath;
};

/** Render `sec` seconds; per 2.7 ms block: f0, voicing, output RMS, narrowest oral area. */
function run(sec: number) {
  const n = Math.round(sec * FS);
  const f0: number[] = [], voicing: number[] = [], rms: number[] = [], minOral: number[] = [];
  for (let i = 0; i < n; i += 128) {
    const m = Math.min(128, n - i);
    const out = voice.render(m);
    let e = 0;
    for (let j = 0; j < m; j++) e += out[j] * out[j];
    rms.push(Math.sqrt(e / m));
    const st = voice.state();
    f0.push(st.f0);
    voicing.push(st.voicing);
    let mn = Infinity;
    for (const a of st.orAreas) mn = Math.min(mn, a);
    minOral.push(mn);
  }
  // the last 40 % of the span, where a note has settled
  const late = <T,>(xs: T[]) => xs.slice(Math.floor(xs.length * 0.6));
  return { f0, voicing, rms, minOral, late };
}
const median = (xs: number[]) => [...xs].sort((a, b) => a - b)[Math.floor(xs.length / 2)];
const cents = (f: number, midi: number) => 1200 * Math.log2(f / midiToHz(midi));

describe("Sing mode, live keys", () => {
  test("each key sings the next syllable at its pitch; legato, melisma and release to silence", () => {
    voice = new WasmVoice(FS);
    useSing.getState().setVibrato(0);
    useSing.getState().setLyrics("la ma -");
    expect(useSing.getState().syllables.map((s) => s.text)).toEqual(["la", "ma", "~"]);
    run(0.1);

    // first key: /la/ on A2
    singNoteOn(45, 96, MALE);
    expect(useSing.getState().cursor).toBe(1);
    const a = run(0.5);
    expect(Math.abs(cents(median(a.late(a.f0)), 45))).toBeLessThan(30);
    expect(Math.min(...a.late(a.voicing))).toBeGreaterThan(0.5);

    // second key pressed while the first is down: legato into /ma/ on C3 (the lips close once)
    singNoteOn(48, 96, MALE);
    const b = run(0.5);
    expect(Math.abs(cents(median(b.late(b.f0)), 48))).toBeLessThan(30);
    expect(Math.min(...b.minOral)).toBeLessThan(0.05); // the /m/ closure
    expect(Math.min(...b.late(b.minOral))).toBeGreaterThan(0.2); // open again on the vowel

    // releasing the first key (no longer sounding) changes nothing
    singNoteOff(45, MALE);
    const c = run(0.2);
    expect(Math.min(...c.voicing)).toBeGreaterThan(0.5);

    // melisma: the vowel carries on to E3 without a consonant
    singNoteOn(52, 96, MALE);
    expect(useSing.getState().cursor).toBe(0); // wrapped round
    const d = run(0.5);
    expect(Math.abs(cents(median(d.late(d.f0)), 52))).toBeLessThan(30);
    expect(Math.min(...d.minOral)).toBeGreaterThan(0.2);
    expect(Math.min(...d.voicing)).toBeGreaterThan(0.5);

    // releasing E3 while C3 is still down glides back to C3 on the same vowel
    singNoteOff(52, MALE);
    const e = run(0.4);
    expect(Math.abs(cents(median(e.late(e.f0)), 48))).toBeLessThan(30);
    expect(Math.min(...e.voicing)).toBeGreaterThan(0.5);

    // last key up: the note ends and the voice falls silent
    singNoteOff(48, MALE);
    const f = run(0.6);
    let peak = 0;
    for (const r of [...a.rms, ...b.rms]) peak = Math.max(peak, r);
    expect(Math.max(...f.rms.slice(Math.floor(0.3 * FS / 128)))).toBeLessThan(peak * 0.003); // < -50 dB
  });

  test("a soft key breathes less than a hard one", () => {
    const level = (velocity: number) => {
      voice = new WasmVoice(FS);
      useSing.getState().setLyrics("ah");
      run(0.05);
      singNoteOn(50, velocity, MALE);
      const r = run(0.5);
      singNoteOff(50, MALE);
      run(0.4);
      return { rms: median(r.late(r.rms)) };
    };
    const soft = level(30), hard = level(127);
    if (process.env.SING_VERBOSE) console.log(`hard vs soft key: ${(20 * Math.log10(hard.rms / soft.rms)).toFixed(1)} dB`);
    expect(hard.rms / soft.rms).toBeGreaterThan(1.6); // > 4 dB
  });

  test("a demo still waiting for audio or the dictionary is cancelled by a stop", async () => {
    voice = new WasmVoice(FS);
    let scheduled = 0;
    const schedule = engineStub.schedule as typeof fakeEngine.schedule;
    engineStub.schedule = (plan: { gestures: Gesture[] }) => (scheduled++, schedule(plan));
    let go!: () => void;
    const waiting = playDemo("Made of air", MALE, () => new Promise<void>((r) => (go = r)));
    stopSinging(); // e.g. the mode changed while audio was starting
    go();
    await waiting;
    expect(scheduled).toBe(0);
    expect(useSing.getState().demo).toBeNull();
    await playDemo("Made of air", MALE, () => Promise.resolve());
    expect(scheduled).toBe(1);
    expect(useSing.getState().demo).toBe("Made of air");
    stopSinging();
    engineStub.schedule = schedule;
  });

  test("a demo shows the song's words while it plays, then gives the player's own back", async () => {
    voice = new WasmVoice(FS);
    const sing = useSing.getState;
    sing().setLyrics("my own words to sing");
    sing().setCursor(2);
    // stopped (a song chip pressed again, the mode or body changed)
    await playDemo("Made of air", MALE, () => Promise.resolve());
    expect(sing().demo).toBe("Made of air");
    expect(sing().lyrics).not.toBe("my own words to sing");
    stopSinging();
    expect(sing().lyrics).toBe("my own words to sing");
    expect(sing().syllables.map((s) => s.text)).toEqual(["my", "own", "words", "to", "sing"]);
    expect(sing().cursor).toBe(2);
    // played to the end
    await playDemo("Made of air", MALE, () => Promise.resolve());
    planned({ type: "planned", id: 1, t0: voice.clock - 1000, end: 0 });
    tickSinging();
    expect(sing().demo).toBeNull();
    expect(sing().lyrics).toBe("my own words to sing");
    expect(sing().cursor).toBe(2);
    // a second song started while the first plays gives back the player's words, not the first song's
    await playDemo("Made of air", MALE, () => Promise.resolve());
    await playDemo("Made of air", MALE, () => Promise.resolve());
    stopSinging();
    expect(sing().lyrics).toBe("my own words to sing");
    // words typed during a demo are the player's from then on
    await playDemo("Made of air", MALE, () => Promise.resolve());
    sing().setLyrics("typed during the song");
    expect(sing().demo).toBeNull();
    expect(sing().lyrics).toBe("typed during the song");
  });

  test("a quick next note drops the last one's breath-out; leaving Sing cancels a pending ending", () => {
    voice = new WasmVoice(FS);
    useSing.getState().setVibrato(0);
    useSing.getState().setLyrics("cat dog");
    run(0.05);
    singNoteOn(50, 96, MALE);
    run(0.4);
    singNoteOff(50, MALE); // "cat": its /t/ and the breath-out are scheduled
    expect(gesturesOf().some((g) => g.label === "exhale")).toBe(true);
    run(0.02);
    singNoteOn(52, 96, MALE); // "dog" straight after
    expect(gesturesOf().some((g) => g.label === "exhale")).toBe(false);
    const d = run(0.4);
    expect(Math.min(...d.late(d.voicing))).toBeGreaterThan(0.5);
    singNoteOff(52, MALE); // "dog": its /g/ ending is scheduled...
    run(0.01);
    stopSinging(); // ...and the mode changes at once
    run(0.06);
    // the cancelled ending's breath is gone (the note's own breath is at the end of its 70 ms fade)
    expect(breathNow()).toBeLessThan(0.01);
  });

  test("overlapping endings all stay owned: a stop cancels every one", () => {
    voice = new WasmVoice(FS);
    useSing.getState().setLyrics("cats dogs");
    run(0.05);
    singNoteOn(50, 96, MALE);
    run(0.3);
    singNoteOff(50, MALE); // "cats": /ts/ and breath-out
    run(0.01);
    singNoteOn(52, 96, MALE);
    run(0.03);
    singNoteOff(52, MALE); // "dogs" released quickly, while the /ts/ is still going
    run(0.01);
    stopSinging();
    run(0.08);
    expect(breathNow()).toBeLessThan(0.01);
  });
});

describe("keys pressed before the audio has started", () => {
  test("only the press still down sings, once, and not after the body changed", async () => {
    const { useStore } = await import("../src/ui/store.ts");
    voice = new WasmVoice(FS);
    useStore.setState({ started: false, mode: "play" });
    useStore.getState().setMode("sing");
    useSing.getState().setLyrics("one two three");
    let scheduled = 0;
    const sg = engineStub.scheduleGestures as typeof fakeEngine.scheduleGestures;
    engineStub.scheduleGestures = (gs: Gesture[], lead?: number) => (scheduled++, sg(gs, lead));
    const st = useStore.getState();
    // press, release and press again while audio starts: one note, one syllable
    st.noteOn(50);
    st.noteOff(50);
    st.noteOn(50);
    startAudio();
    await new Promise((r) => setTimeout(r, 0));
    await new Promise((r) => setTimeout(r, 0));
    expect(useStore.getState().started).toBe(true);
    expect(scheduled).toBe(1);
    expect(useSing.getState().cursor).toBe(1);
    useStore.getState().noteOff(50);

    // a press waiting for audio is dropped by a body change
    useStore.setState({ started: false });
    scheduled = 0;
    const before = useSing.getState().cursor;
    useStore.getState().noteOn(52);
    useStore.getState().setBody({ female: 1, child: 0 });
    startAudio();
    await new Promise((r) => setTimeout(r, 0));
    await new Promise((r) => setTimeout(r, 0));
    expect(scheduled).toBe(0);
    expect(useSing.getState().cursor).toBe(before);
    engineStub.scheduleGestures = sg;
  });
});
