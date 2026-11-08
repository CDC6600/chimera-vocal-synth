/**
 * The stores' view of the plugin: filled from one snapshot before the first render, then kept up by
 * polling (docs/plugin.md, "One owner for each value"). Only the plugin's editor imports this.
 */
import { engine } from "../audio/AudioEngine.ts";
import { ARTIC_PARAMS, SOURCE_PARAMS, dbToVolume, gestureOpen, setRevision } from "../audio/pluginBridge.ts";
import type { BeatStep, BodyMix, HumanArtic, Syllable, VoiceSource } from "../wasm/engine.ts";
import { TELE_LABELS, decodeTelemetry } from "../worklet/telemetry.ts";
import { clearPluginNotes, followPluginNote, roll, useSing } from "./singing.ts";
import { followSpoken, useSpeech } from "./speech.ts";
import { useStore } from "./store.ts";

/** Scalars at the head of a `/poll` response: POLL_HEADER in crates/chimera-plugin/src/bridge.rs. */
const POLL_HEADER = 25;
/** Parameters are read every this many frames: about 20 times a second. */
const PARAMS_EVERY = 3;
/** The engine-load readout's window (ms). */
const LOAD_WINDOW = 500;

type Params = Record<string, number>;

interface Snapshot {
  revision: number;
  syllablesVersion: number;
  settings: { lyrics: string; body: BodyMix; mode: "sing" | "play" | "speak" | "beatbox"; artic: Partial<HumanArtic>; tipFree: boolean; voiced: boolean; f0: number; speech: { text: string; ipa: boolean; rate: number }; beat: { steps: (BeatStep | null)[]; bpm: number; swing: number; preset: string | null; mic: boolean; follow: boolean } };
  params: Params;
  syllables: Syllable[];
  cursor: number;
  speech: SpeechLines;
  speechCursor: number;
  sampleRate: number;
}

interface SpeechLines {
  generation: number;
  lines: string[];
}

async function getJson<T>(path: string): Promise<T> {
  const r = await fetch(path, { cache: "no-store" });
  if (!r.ok) throw new Error(`${path}: HTTP ${r.status}`);
  return (await r.json()) as T;
}

let revision = -1;
let syllablesVersion = -1;
let linesGeneration = -1;

/** The whole state, as a project was saved or restored: nothing the page held survives this. */
function adopt(s: Snapshot): void {
  revision = s.revision;
  setRevision(s.revision);
  syllablesVersion = s.syllablesVersion;
  if (s.sampleRate > 0) engine.sampleRate = s.sampleRate;
  const initial = useStore.getInitialState();
  const source = { ...initial.source, f0: s.settings.f0, voiced: s.settings.voiced } as VoiceSource;
  for (const [key, id] of Object.entries(SOURCE_PARAMS)) (source as unknown as Params)[key] = s.params[id];
  const artic = { ...initial.artic, ...s.settings.artic } as HumanArtic;
  for (const [key, id] of Object.entries(ARTIC_PARAMS)) (artic as unknown as Params)[key] = s.params[id];
  const { mic, ...beat } = s.settings.beat;
  useStore.setState({ started: true, mode: s.settings.mode, body: s.settings.body, source, artic, tipFree: s.settings.tipFree, volume: dbToVolume(s.params.gain), hold: false, heldNotes: [], mic, beat: { ...beat, playing: false, following: false, hostBpm: 0 } });
  useSing.setState({ lyrics: s.settings.lyrics, syllables: s.syllables, cursor: s.cursor, vibrato: s.params.vibrato });
  linesGeneration = s.speech.generation;
  const { text, ipa, rate } = s.settings.speech;
  useSpeech.setState({ text, ipa, rate, lines: s.speech.lines, cursor: s.speechCursor });
}

/** Host values, for the parameters not being moved here. */
function followParams(p: Params): void {
  const st = useStore.getState();
  let source: Partial<VoiceSource> | null = null;
  for (const [key, id] of Object.entries(SOURCE_PARAMS)) {
    if (!gestureOpen(id) && p[id] !== (st.source as unknown as Params)[key]) (source ??= {} as Partial<VoiceSource>)[key as "fry"] = p[id];
  }
  let artic: Partial<HumanArtic> | null = null;
  for (const [key, id] of Object.entries(ARTIC_PARAMS)) {
    if (!gestureOpen(id) && p[id] !== (st.artic as unknown as Params)[key]) (artic ??= {} as Partial<HumanArtic>)[key as "epilarynx"] = p[id];
  }
  const volume = dbToVolume(p.gain);
  const patch: Partial<ReturnType<typeof useStore.getState>> = {};
  if (source) patch.source = { ...st.source, ...source };
  if (artic) patch.artic = { ...st.artic, ...artic };
  if (!gestureOpen("gain") && volume !== st.volume) patch.volume = volume;
  if (Object.keys(patch).length) useStore.setState(patch);
  if (!gestureOpen("vibrato") && p.vibrato !== useSing.getState().vibrato) useSing.setState({ vibrato: p.vibrato });
}

let analysers: ReturnType<typeof engine.pluginAnalysers> | null = null;
let busy = 0, audio = 0, loadAt = 0;
let frames = 0;
let shownNote = -1;
/** the telemetry's voice clock last seen: it starts again with a new voice */
let lastClock = 0;

async function poll(): Promise<void> {
  const r = await fetch("/poll", { cache: "no-store" });
  const buf = await r.arrayBuffer();
  const h = new Float64Array(buf, 0, POLL_HEADER);
  const [rev, syllables, cursor, sung, note, serial, busyNs, audioNs, , faulted, , female, child, spokenSerial, spokenLine, spokenGeneration, spokenT0, lines, speechCursor, beatStep, beatRunning, beatFollowing, hostBpm, samples, n] = h;
  if (n > 0) {
    const { t, nPh, nOr } = decodeTelemetry(new Float64Array(buf, 8 * POLL_HEADER, n), new Uint8Array(buf, 8 * (POLL_HEADER + n), TELE_LABELS));
    // a new voice's clock starts from zero: what's on the roll belongs to the old timeline
    if (t.clock < lastClock) clearPluginNotes();
    lastClock = t.clock;
    engine.receive({ type: "telemetry", t, load: engine.load, body: { female, child }, nPh, nOr, beatStep });
  }
  // the output's samples, after the telemetry and its labels; a slice, because they may not be aligned
  if (samples > 0) {
    const at = 8 * (POLL_HEADER + n) + (n > 0 ? TELE_LABELS : 0);
    analysers?.feed(new Float32Array(buf.slice(at, at + 4 * samples)));
  }
  busy += busyNs;
  audio += audioNs;
  const now = performance.now();
  if (now - loadAt > LOAD_WINDOW && audio > 0) {
    engine.load = busy / audio;
    busy = audio = 0;
    loadAt = now;
  }
  engine.error = faulted ? "The voice engine stopped. Reload the plugin to start it again." : null;

  // a restore replaced everything
  if (rev !== revision) {
    adopt(await getJson<Snapshot>("/state"));
    shownNote = -1;
    return;
  }
  if (syllables !== syllablesVersion) {
    syllablesVersion = syllables;
    useSing.setState({ syllables: await getJson<Syllable[]>("/syllables") });
  }
  if (cursor !== useSing.getState().cursor) useSing.setState({ cursor });
  if (lines !== linesGeneration) {
    linesGeneration = lines;
    useSpeech.setState({ lines: (await getJson<SpeechLines>("/speech")).lines });
  }
  if (speechCursor !== useSpeech.getState().cursor) useSpeech.setState({ cursor: speechCursor });
  void followSpoken(spokenSerial, spokenLine, spokenGeneration, spokenT0);
  const beat = useStore.getState().beat;
  if (beat.playing !== !!beatRunning || beat.following !== !!beatFollowing || beat.hostBpm !== hostBpm) useStore.setState({ beat: { ...beat, playing: !!beatRunning, following: !!beatFollowing, hostBpm } });
  const clock = engine.voiceClock() ?? 0;
  followPluginNote(note, sung, serial, clock);
  if (note !== shownNote) {
    shownNote = note;
    // the keys set the pitch in Play: the encoder and the readout follow
    const st = useStore.getState();
    if (note >= 0 && st.mode === "play") useStore.setState({ source: { ...st.source, f0: 440 * Math.pow(2, (note - 69) / 12) } });
  }
  if (++frames % PARAMS_EVERY === 0) followParams(await getJson<Params>("/params"));
}

function loop(): void {
  poll()
    .catch((e) => console.error("plugin poll:", e))
    .finally(() => requestAnimationFrame(loop));
}

/** Fill the stores from the plugin and start following it. Await before the first render. */
export async function attachToPlugin(): Promise<void> {
  // for the end-to-end test: what the piano roll holds isn't in the DOM
  (window as unknown as { __chimeraRoll: typeof roll }).__chimeraRoll = roll;
  adopt(await getJson<Snapshot>("/state"));
  analysers = engine.pluginAnalysers();
  requestAnimationFrame(loop);
}
