/**
 * Sing mode on the main thread. The lyrics become syllables with a cursor; each key sings the
 * next syllable at its pitch. A key pressed while the previous one is still down sings legato
 * (the previous syllable's coda runs straight into the next onset); a "-" in the lyrics holds
 * the vowel onto a new pitch (melisma). Releasing the sounding key while another is still down
 * glides back to that key on the same vowel. Key velocity sets the breath.
 * Demo songs are planned whole (planSong) and sent as one score.
 */
import { create } from "zustand";
import { engine } from "../audio/AudioEngine.ts";
import { g2p } from "./speech.ts";
import { lyricsToSyllables, type Syllable } from "../wasm/engine.ts";
import { planSong, planNoteOn, planNoteOff, planMelisma } from "../wasm/engine.ts";
import { EXHALE_LABEL } from "../wasm/engine.ts";
import { DEMO_SONGS } from "../wasm/engine.ts";
import type { Gesture } from "../wasm/types.ts";
import { nearestBody, type BodyMix } from "./body.ts";

/** A note on the piano roll, on the voice clock (s); t1 is null while it is held. */
export interface RollNote {
  midi: number;
  t0: number;
  t1: number | null;
  text: string;
  demo: boolean;
}

/** What the piano roll draws, read every frame (mutated in place: no re-render per note). */
export const roll: { notes: RollNote[] } = { notes: [] };

export const DEFAULT_LYRICS = "Hello there, I am a voice made of air.";

/** sung when the lyrics are empty */
const AH: Syllable = { text: "ah", onset: [], nucleus: { ipa: "ɑ", stress: 1 }, coda: [], wordEnd: true, phraseEnd: false };

interface SingState {
  lyrics: string;
  syllables: Syllable[];
  /** the syllable the next key sings */
  cursor: number;
  /** vibrato depth on held vowels (cents) */
  vibrato: number;
  /** the demo song playing, if any */
  demo: string | null;
  /** index of the demo syllable being sung (−1: none yet) */
  demoAt: number;
  setLyrics: (text: string) => void;
  setCursor: (i: number) => void;
  setVibrato: (cents: number) => void;
}

export const useSing = create<SingState>((set, get) => ({
  lyrics: DEFAULT_LYRICS,
  syllables: lyricsToSyllables(DEFAULT_LYRICS),
  cursor: 0,
  vibrato: 35,
  demo: null,
  demoAt: -1,
  setLyrics: (text) => {
    stopDemo();
    set({ lyrics: text, syllables: lyricsToSyllables(text), cursor: 0 });
  },
  setCursor: (i) => set({ cursor: Math.max(0, Math.min(i, Math.max(0, get().syllables.length - 1))) }),
  setVibrato: (cents) => set({ vibrato: cents }),
}));

/** Load the pronouncing dictionary, then re-read the lyrics with it (the cursor stays). */
export function ensureDictionary(): void {
  if (g2p.loaded) return;
  g2p
    .load()
    .then(() => {
      const s = useSing.getState();
      const syllables = lyricsToSyllables(s.lyrics);
      useSing.setState({ syllables, cursor: Math.min(s.cursor, Math.max(0, syllables.length - 1)) });
    })
    .catch(() => {
      // no dictionary: the letter-to-sound rules keep working
    });
}

// ---- live notes

interface Sounding {
  midi: number;
  /** the syllable whose vowel is sounding (a melisma keeps its parent's, for the coda) */
  syl: Syllable;
  tag: number;
  note: RollNote;
}

let sounding: Sounding | null = null;

/** How a syllable is written at the mouth: a hyphen when its word goes on. */
const written = (s: Syllable) => s.text + (s.wordEnd ? "" : "-");

/**
 * The syllable being sung now, for the words at the mouth: the demo's note under the clock, or
 * the held key's. A melisma keeps its syllable's key, so the word stays while the pitch moves.
 */
export function sungNow(): { key: unknown; text: string } | null {
  const d = demoRun;
  if (d) {
    if (d.offset === null) return null;
    const t = now() - d.offset;
    // notes follow one another without a gap, so a melisma's syllable stays put
    const k = d.times.findIndex(([a, b]) => t >= a && t < b);
    return k < 0 ? null : { key: `${d.id}:${d.parent[k]}`, text: d.shown[k] };
  }
  return sounding ? { key: sounding, text: written(sounding.syl) } : null;
}

/** The syllable whose vowel is sounding from the keys now (null when no key sings). */
export function soundingSyllable(): string | null {
  return sounding?.syl.text ?? null;
}
/** keys down, oldest first */
let keys: number[] = [];
/** gesture tags: a note uses tag (articulation, breath) and tag + 1 (pitch, vibrato) */
let nextTag = 2;
/** endings (glide, coda) that may still be playing: tag, and tag + 1 for the breath-out */
const tails: number[] = [];
/** endings whose breath-out has not been dropped by a following note */
let exhalesLive = false;

/** breath level for a key velocity (1 at the on-screen keys' default of 96) */
const breathFor = (velocity: number) => 0.62 + 0.4 * Math.min(1, Math.max(0, velocity / 96)) + 0.12 * Math.max(0, (velocity - 96) / 31);

const now = () => engine.voiceClock() ?? 0;

function addNote(midi: number, t0: number, text: string, demo = false, t1: number | null = null): RollNote {
  const n: RollNote = { midi, t0, t1, text, demo };
  roll.notes.push(n);
  // keep a minute of history
  const cut = t0 - 60;
  if (roll.notes.length > 400 || (roll.notes[0] && (roll.notes[0].t1 ?? t0) < cut)) roll.notes = roll.notes.filter((x) => (x.t1 ?? t0) >= cut).slice(-400);
  return n;
}

/** The syllable a melisma at index i continues. */
function parentOf(syls: Syllable[], i: number): Syllable {
  for (let k = i; k >= 0; k--) if (!syls[k].melisma) return syls[k];
  return AH;
}

function startNote(syl: Syllable, midi: number, velocity: number, body: BodyMix, text: string): void {
  const t = now();
  const tag = nextTag;
  nextTag += 2;
  // earlier codas may finish, but their breath-out must not open the folds on this onset
  if (exhalesLive) for (const t of tails) engine.release(t + 1);
  exhalesLive = false;
  const from = sounding;
  if (from) {
    engine.release(from.tag);
    engine.release(from.tag + 1);
    from.note.t1 = t;
  }
  const gs: Gesture[] = planNoteOn(syl, midi, { body, vibrato: useSing.getState().vibrato, tag, legatoFrom: from ? { syl: from.syl, midi: from.midi } : undefined });
  const breath = breathFor(velocity);
  for (const g of gs) if (g.targets.breath !== undefined) g.targets = { ...g.targets, breath: g.targets.breath * breath };
  engine.scheduleGestures(gs);
  sounding = { midi, syl, tag, note: addNote(midi, t, text) };
}

/** The held vowel moves to a new pitch (melisma, or back to a key still down). */
function glide(midi: number, text: string): void {
  const s = sounding!;
  const t = now();
  engine.release(s.tag + 1);
  engine.scheduleGestures(planMelisma(midi, { vibrato: useSing.getState().vibrato, tag: s.tag }));
  s.note.t1 = t;
  s.midi = midi;
  s.note = addNote(midi, t, text);
}

export function singNoteOn(midi: number, velocity: number, body: BodyMix): void {
  stopDemo();
  keys = keys.filter((k) => k !== midi).concat(midi);
  const st = useSing.getState();
  const syls = st.syllables.length ? st.syllables : [AH];
  const i = st.cursor % syls.length;
  const syl = syls[i];
  if (syl.melisma && sounding) glide(midi, syl.text);
  // a melisma with nothing held: sing its vowel again, without the onset
  else startNote(syl.melisma ? { ...parentOf(syls, i), onset: [] } : syl, midi, velocity, body, syl.text);
  useSing.setState({ cursor: (i + 1) % syls.length });
}

export function singNoteOff(midi: number, body: BodyMix): void {
  keys = keys.filter((k) => k !== midi);
  const s = sounding;
  if (!s || s.midi !== midi) return;
  if (keys.length) {
    glide(keys[keys.length - 1], "~");
    return;
  }
  engine.release(s.tag);
  engine.release(s.tag + 1);
  s.note.t1 = now();
  sounding = null;
  // a melisma comes next: this vowel carries on there, and the coda waits for the chain's end
  const st = useSing.getState();
  if (st.syllables[st.cursor]?.melisma) return;
  const gs = planNoteOff(s.syl, midi, body);
  if (!gs.length) return;
  const tag = nextTag;
  nextTag += 2;
  engine.scheduleGestures(gs.map((g) => ({ ...g, tag: g.label === EXHALE_LABEL ? tag + 1 : tag })));
  tails.push(tag);
  if (tails.length > 8) tails.shift(); // long finished by now
  exhalesLive = true;
}

// ---- demo songs

interface DemoRun {
  id: number;
  /** note start/end times in plan time (s) */
  times: [number, number][];
  midi: number[];
  text: string[];
  /** what the mouth shows per note, and the note whose syllable it is (a melisma's parent) */
  shown: string[];
  parent: number[];
  end: number;
  /** voice clock of plan time 0, once the worklet has it */
  offset: number | null;
}

let demoRun: DemoRun | null = null;
/** bumped by every stop: a demo still waiting for audio or the dictionary then gives up */
let demoToken = 0;
/**
 * The player's own lyrics and cursor while a demo shows the song's words: given back when the
 * demo ends or stops, so a demo never costs anyone the words they typed.
 */
let ownWords: { lyrics: string; cursor: number } | null = null;

function endDemo(): void {
  demoRun = null;
  const w = ownWords;
  ownWords = null;
  if (!w) return void useSing.setState({ demo: null, demoAt: -1 });
  const syllables = lyricsToSyllables(w.lyrics);
  useSing.setState({ demo: null, demoAt: -1, lyrics: w.lyrics, syllables, cursor: Math.min(w.cursor, Math.max(0, syllables.length - 1)) });
}

engine.onPlanned((m) => {
  const d = demoRun;
  if (!d || m.id !== d.id) return;
  d.offset = m.t0;
  d.times.forEach(([a, b], k) => addNote(d.midi[k], m.t0 + a, d.text[k], true, m.t0 + b - 0.02));
});

/**
 * Sing a demo song (after `ready`, e.g. audio startup, resolves; unless stopped meanwhile). The
 * lyrics show the song's words while it plays; the player's own come back when it ends or stops.
 */
export async function playDemo(name: string, body: BodyMix, ready?: () => Promise<void>): Promise<void> {
  const song = DEMO_SONGS.find((s) => s.name === name);
  if (!song) return;
  stopSinging();
  const token = demoToken;
  await ready?.();
  try {
    await g2p.load();
  } catch {
    // letter-to-sound rules
  }
  if (token !== demoToken) return;
  const syl = lyricsToSyllables(song.lyrics);
  if (!syl.length) return;
  const shift = song.octave[nearestBody(body)] * 12;
  const notes = song.notes.map((n, i) => ({ midi: n[0] + shift, beats: n[1], syl: syl[i % syl.length] }));
  const t0 = 0.35;
  const plan = planSong(notes, { body, bpm: song.bpm, t0, vibrato: useSing.getState().vibrato });
  const beat = 60 / song.bpm;
  let t = t0;
  const times = notes.map((n): [number, number] => {
    const a = t;
    t += n.beats * beat;
    return [a, t];
  });
  const id = engine.schedule({ gestures: plan.gestures, segments: plan.segments, end: plan.end });
  const parent: number[] = [];
  notes.forEach((n, i) => parent.push(n.syl.melisma && i > 0 ? parent[i - 1] : i));
  const shown = parent.map((p) => written(notes[p].syl));
  demoRun = { id, times, midi: notes.map((n) => n.midi), text: notes.map((n) => n.syl.text), shown, parent, end: plan.end, offset: null };
  const s = useSing.getState();
  ownWords ??= { lyrics: s.lyrics, cursor: s.cursor };
  useSing.setState({ lyrics: song.lyrics, syllables: syl, cursor: 0, demo: song.name, demoAt: -1 });
}

function stopDemo(): void {
  demoToken++;
  const d = demoRun;
  if (!d) return;
  engine.stopSpeech();
  // notes not yet sung leave the roll; the one sounding ends now
  const t = now();
  roll.notes = roll.notes.filter((n) => !(n.demo && n.t0 > t));
  for (const n of roll.notes) if (n.demo && n.t1 !== null && n.t1 > t) n.t1 = t;
  endDemo();
}

/** Called every frame while Sing mode is shown: follow the demo's progress. */
export function tickSinging(): void {
  const d = demoRun;
  if (!d || d.offset === null) return;
  const t = now() - d.offset;
  if (t > d.end + 0.4) return endDemo();
  let k = -1;
  while (k + 1 < d.times.length && d.times[k + 1][0] <= t) k++;
  if (k !== useSing.getState().demoAt) useSing.setState({ demoAt: k });
}

/** Stop everything sung (leaving Sing mode, changing body): demo, notes and endings. */
export function stopSinging(): void {
  stopDemo();
  if (sounding) {
    engine.release(sounding.tag);
    engine.release(sounding.tag + 1);
    sounding.note.t1 = now();
    sounding = null;
  }
  for (const t of tails) {
    engine.release(t);
    engine.release(t + 1);
  }
  tails.length = 0;
  exhalesLive = false;
  keys = [];
}

export { DEMO_SONGS };
