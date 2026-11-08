/**
 * CHIMERA AudioWorklet processor: the Rust engine (wasm) on the audio thread.
 *
 * Messages map to engine calls as they arrive (between render quanta, on this thread), so their
 * order is the order they take effect. process() renders, measures load and posts fixed-size
 * telemetry (~60 Hz). If the engine traps (a bug), the processor outputs silence from then on and
 * reports once; the main thread may build a new node, which restarts playback state.
 */
import { writeGestures, writeSteps, fromUtf8, utf8, CONTROL_KEYS } from "../wasm/wire.ts";
import type { ToWorklet, FromWorklet, TelemetryMessage } from "./protocol.ts";
import type { BodyMix, CreatureTelemetry } from "../wasm/types.ts";

// Minimal AudioWorkletGlobalScope typings
declare const sampleRate: number;
declare function registerProcessor(name: string, ctor: unknown): void;
declare class AudioWorkletProcessor {
  readonly port: MessagePort;
  constructor(options?: unknown);
}

type Fn = (...a: number[]) => number;
type Exports = { memory: WebAssembly.Memory } & Record<string, Fn>;

const ARTIC_KEYS = CONTROL_KEYS.slice(0, 17);
const SOURCE_KEYS = ["f0", "glide", "pressure", "breathiness", "tension", "fry", "roughness", "vibratoDepth", "vibratoRate", "drift", "voiced", "brightness"];
/** scalars at the head of the human telemetry block (crates/chimera-wasm/src/lib.rs) */
const TELE_HEADER = 16;
/** gestures taken from one schedule message */
const MAX_GESTURES = 4000;

/** What the engine says about the creature it switched to (crates/chimera-wasm/src/lib.rs). */
interface CreatureInfo {
  id: string;
  params: string[];
  calls: string[];
  tubes: { name: string; n: number }[];
  sources: string[];
}

class ChimeraProcessor extends AudioWorkletProcessor {
  private readonly x: Exports;
  private readonly h: number;
  private failed = false;
  private telemetryOn = true;
  private blocks = 0;
  private load = 0;
  private creature: CreatureInfo | null = null;

  constructor(options: { processorOptions: { module: WebAssembly.Module } }) {
    super();
    this.x = new WebAssembly.Instance(options.processorOptions.module, {}).exports as unknown as Exports;
    this.h = this.x.engine_new(sampleRate);
    this.port.onmessage = (e: MessageEvent<ToWorklet>) => {
      if (this.failed) return;
      try {
        this.handle(e.data);
      } catch (err) {
        this.fail(err);
      }
    };
    this.postReady();
  }

  private post(msg: FromWorklet): void {
    this.port.postMessage(msg);
  }

  private fail(err: unknown): void {
    if (this.failed) return;
    this.failed = true;
    this.post({ type: "error", message: `engine stopped: ${String(err)}` });
  }

  private get mem(): ArrayBuffer {
    return this.x.memory.buffer;
  }

  private body(): BodyMix {
    return { female: this.x.engine_body_female(this.h), child: this.x.engine_body_child(this.h) };
  }

  private postReady(): void {
    const x = this.x, h = this.h;
    this.post({ type: "ready", sampleRate, body: this.body(), nPh: x.engine_n_ph(h), nOr: x.engine_n_or(h), creature: this.creature?.id ?? "human" });
  }

  /** A JSON call on this instance's engine (non-real-time: creature switches). */
  private engineJson<T>(fn: string, args: Record<string, unknown>): T {
    const x = this.x;
    const bytes = utf8(JSON.stringify(args));
    const p = x.chimera_alloc(bytes.length);
    new Uint8Array(this.mem, p, bytes.length).set(bytes);
    const status = x[fn](this.h, p, bytes.length);
    x.chimera_free(p, bytes.length);
    const v = JSON.parse(fromUtf8(new Uint8Array(this.mem, x.chimera_result_ptr(), x.chimera_result_len())));
    if (status !== 0) throw new Error(v.error);
    return v as T;
  }

  /** The engine's value area, cleared to NaN ("unchanged"). */
  private values(): Float64Array {
    return new Float64Array(this.mem, this.x.engine_values(this.h), 32).fill(NaN);
  }

  private handle(m: ToWorklet): void {
    const x = this.x, h = this.h;
    switch (m.type) {
      case "body":
        x.engine_set_body(h, m.body.female, m.body.child);
        this.postReady();
        break;
      case "artic": {
        const v = this.values();
        for (const [k, val] of Object.entries(m.values)) {
          const i = ARTIC_KEYS.indexOf(k as never);
          if (i >= 0 && typeof val === "number") v[i] = val;
        }
        x.engine_set_artic(h, m.tipFree === undefined ? 2 : m.tipFree ? 1 : 0);
        break;
      }
      case "source": {
        const v = this.values();
        for (const [k, val] of Object.entries(m.values)) {
          const i = SOURCE_KEYS.indexOf(k);
          if (i >= 0) v[i] = typeof val === "boolean" ? (val ? 1 : 0) : (val as number);
        }
        x.engine_set_source(h);
        break;
      }
      case "gate":
        x.engine_set_gate(h, m.level);
        break;
      case "telemetry":
        this.telemetryOn = m.enabled;
        break;
      case "schedule": {
        // planning happened on the main thread; the engine shifts the gestures onto its clock
        const n = writeGestures(this.mem, x.engine_records(h), m.gestures, MAX_GESTURES);
        const off = x.engine_schedule(h, n, m.lead ?? 0.03, m.replace ? 1 : 0);
        if (m.segments.length) this.post({ type: "planned", id: m.id, t0: off, end: m.end + off, segments: m.segments.map((s) => ({ ...s, t0: s.t0 + off, t1: s.t1 + off })) });
        break;
      }
      case "stopSpeech":
        x.engine_stop_speech(h);
        break;
      case "release":
        x.engine_release(h, m.tag);
        break;
      case "beat": {
        const n = m.steps ? writeSteps(this.mem, x.engine_records(h), m.steps) : 0;
        x.engine_set_beat(h, m.steps ? 1 : 0, n, m.bpm ?? NaN, m.swing ?? NaN, m.playing === undefined ? 2 : m.playing ? 1 : 0);
        break;
      }
      case "beatHit":
        writeSteps(this.mem, x.engine_records(h), [{ id: m.id, note: m.note, accent: m.accent }]);
        x.engine_beat_hit(h);
        break;
      case "creature":
        this.creature = m.id === "human" ? (x.engine_set_human(h), null) : this.engineJson<CreatureInfo>("engine_set_creature", { id: m.id, params: m.params ?? {} });
        this.postReady();
        break;
      case "cparams": {
        const c = this.creature;
        if (!c) break;
        for (const [k, val] of Object.entries(m.values)) {
          const i = c.params.indexOf(k);
          if (i >= 0) x.engine_creature_param(h, i, val);
        }
        break;
      }
      case "call": {
        const i = this.creature ? this.creature.calls.indexOf(m.id) : -1;
        if (i >= 0) x.engine_play_call(h, i, m.pitch ?? NaN, m.amp ?? NaN);
        break;
      }
      case "stopCall":
        x.engine_stop_call(h);
        break;
    }
  }

  process(_inputs: Float32Array[][], outputs: Float32Array[][]): boolean {
    const out = outputs[0];
    if (!out || out.length === 0) return true;
    const ch0 = out[0];
    if (this.failed) {
      for (const ch of out) ch.fill(0);
      return true;
    }
    const t0 = Date.now();
    try {
      const p = this.x.engine_render(this.h, ch0.length);
      ch0.set(new Float32Array(this.mem, p, ch0.length));
    } catch (err) {
      ch0.fill(0);
      this.fail(err);
    }
    for (let c = 1; c < out.length; c++) out[c].set(ch0);
    const dt = Date.now() - t0;
    const budget = (ch0.length / sampleRate) * 1000;
    this.load += 0.05 * (dt / budget - this.load);
    if (this.telemetryOn && !this.failed && ++this.blocks % 6 === 0) {
      try {
        if (this.creature) this.post({ type: "ctelemetry", t: this.creatureTelemetry(this.creature), load: this.load });
        else this.post(this.humanTelemetry());
      } catch (err) {
        this.fail(err);
      }
    }
    return true;
  }

  /** The human voice's telemetry block → the message the UI reads. */
  private humanTelemetry(): TelemetryMessage {
    const x = this.x, h = this.h;
    const p = x.engine_telemetry(h);
    const [phLen, orLen, velumPort, f0, voicing, pLung, outRms, noiseLevel, noiseNode, gate, clock, nArtic, nPh, nOr, nScope, nActive] = new Float64Array(this.mem, p, TELE_HEADER);
    const all = new Float64Array(this.mem, p, TELE_HEADER + nArtic + 3 * (nPh + nOr) + 2 * nScope);
    let o = TELE_HEADER;
    const take = (n: number) => Float32Array.from(all.subarray(o, (o += n)));
    const artic = take(nArtic), phAreas = take(nPh), orAreas = take(nOr), pressureRms = take(nPh + nOr), flow = take(nPh + nOr), glottalArea = take(nScope), glottalFlow = take(nScope);
    const labels = new Uint8Array(this.mem, x.engine_tele_labels(h), 4 * 25);
    const active: string[] = [];
    for (let i = 0; i < nActive; i++) active.push(fromUtf8(labels.subarray(i * 25 + 1, i * 25 + 1 + labels[i * 25])));
    return {
      type: "telemetry",
      t: { artic, phAreas, orAreas, phLen, orLen, pressureRms, flow, velumPort, glottalArea, glottalFlow, f0, voicing, pLung, outRms, noiseLevel, noiseNode, gate, clock, active },
      load: this.load,
      body: this.body(),
      nPh,
      nOr,
      beatStep: x.engine_beat_step(h),
    };
  }

  /** A creature's telemetry block → the message the UI reads (names from the switch's info). */
  private creatureTelemetry(c: CreatureInfo): CreatureTelemetry {
    const x = this.x, h = this.h;
    const len = 4 + c.params.length + c.tubes.reduce((s, t) => s + 2 * t.n, 0) + c.sources.length;
    const b = new Float64Array(this.mem, x.engine_creature_telemetry(h), len);
    const [gate, outRms, clock, call] = b;
    let o = 4;
    const params: Record<string, number> = {};
    for (const k of c.params) params[k] = b[o++];
    const tubes: CreatureTelemetry["tubes"] = {};
    for (const t of c.tubes) {
      const areas = Float32Array.from(b.subarray(o, (o += t.n)));
      const rms = Float32Array.from(b.subarray(o, (o += t.n)));
      tubes[t.name] = { areas, rms };
    }
    const source: Record<string, number> = {};
    for (const k of c.sources) source[k] = b[o++];
    return { kind: "creature", id: c.id, params, gate, outRms, clock, tubes, source, call: call >= 0 ? c.calls[call] : null };
  }
}

registerProcessor("chimera", ChimeraProcessor);
