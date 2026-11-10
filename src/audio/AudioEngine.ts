/**
 * Main-thread side of the audio engine: AudioContext + worklet node + analyser, coalesced
 * parameter messages (at most one per animation frame per kind) and telemetry fan-out.
 */
import workletUrl from "../worklet/processor.ts?worker&url";
import { chimeraModule } from "../wasm/chimera.ts";
import type { ToWorklet, FromWorklet, TelemetryMessage, PlannedMessage, CreatureTelemetryMessage } from "../worklet/protocol.ts";
import type { BodyMix } from "../wasm/types.ts";
import type { HumanArtic } from "../wasm/types.ts";
import type { VoiceSource } from "../wasm/types.ts";

type Listener = (t: TelemetryMessage) => void;

export class AudioEngine {
  ctx: AudioContext | null = null;
  private node: AudioWorkletNode | null = null;
  analyser: AnalyserNode | null = null;
  /** unsmoothed, short-window analyser for the spectrogram */
  analyserFast: AnalyserNode | null = null;
  /** long time-domain window for the waveform scope */
  analyserScope: AnalyserNode | null = null;
  private master: GainNode | null = null;
  /** "close mic" chain: proximity bass, presence, compression (beatboxers cup the mic) */
  private micIn: AudioNode | null = null;
  private micOn = false;
  latest: TelemetryMessage | null = null;
  /** performance.now() when `latest` arrived */
  private latestAt = 0;
  latestCreature: CreatureTelemetryMessage | null = null;
  /** load of the most recent telemetry of either kind */
  load = 0;
  sampleRate = 48000;
  private listeners = new Set<Listener>();
  private pendingArtic: Partial<HumanArtic> | null = null;
  private pendingTipFree: boolean | undefined;
  private pendingSource: Partial<VoiceSource> | null = null;
  private pendingCParams: Record<string, number> | null = null;
  private flushScheduled = false;
  private readyListeners = new Set<() => void>();
  private plannedListeners = new Set<(p: PlannedMessage) => void>();
  /** most recent planned utterance (for timelines) */
  lastPlan: PlannedMessage | null = null;
  private speakId = 0;
  error: string | null = null;

  /** The human voice's clock now (s), extrapolated from the latest telemetry; null if none. */
  voiceClock(): number | null {
    if (!this.latest) return null;
    return this.latest.t.clock + Math.min(0.25, (performance.now() - this.latestAt) / 1000);
  }

  get running(): boolean {
    return !!this.ctx && this.ctx.state === "running";
  }

  async start(): Promise<void> {
    if (this.ctx) {
      await this.ctx.resume();
      return;
    }
    // 48 kHz whatever the device runs at (the browser resamples): the airway's cost grows with
    // the square of its rate, and a 192 kHz device took ~3x the CPU of 48 kHz and starved the
    // worklet (load 120-150%, no sound; issue #33)
    const ctx = new AudioContext({ latencyHint: "interactive", sampleRate: 48000 });
    this.ctx = ctx;
    this.sampleRate = ctx.sampleRate;
    await ctx.audioWorklet.addModule(workletUrl);
    // the worklet instantiates the engine from the module compiled at startup (main.tsx)
    const node = new AudioWorkletNode(ctx, "chimera", { numberOfInputs: 0, numberOfOutputs: 1, outputChannelCount: [2], processorOptions: { module: chimeraModule() } });
    this.node = node;
    const master = ctx.createGain();
    master.gain.value = 0.8;
    this.master = master;
    const analyser = ctx.createAnalyser();
    analyser.fftSize = 4096;
    analyser.smoothingTimeConstant = 0.6;
    this.analyser = analyser;
    const fast = ctx.createAnalyser();
    fast.fftSize = 2048;
    fast.smoothingTimeConstant = 0;
    this.analyserFast = fast;
    const scope = ctx.createAnalyser();
    scope.fftSize = 16384;
    this.analyserScope = scope;
    // close-mic chain: proximity effect (low shelf), presence peak, fast compressor, make-up
    const shelf = ctx.createBiquadFilter();
    shelf.type = "lowshelf";
    shelf.frequency.value = 120;
    shelf.gain.value = 9;
    const presence = ctx.createBiquadFilter();
    presence.type = "peaking";
    presence.frequency.value = 3500;
    presence.Q.value = 0.8;
    presence.gain.value = 3;
    const comp = ctx.createDynamicsCompressor();
    comp.threshold.value = -28;
    comp.knee.value = 8;
    comp.ratio.value = 5;
    comp.attack.value = 0.002;
    comp.release.value = 0.12;
    const makeup = ctx.createGain();
    makeup.gain.value = 2.2;
    shelf.connect(presence).connect(comp).connect(makeup).connect(master);
    this.micIn = shelf;
    node.connect(this.micOn ? shelf : master);
    master.connect(ctx.destination);
    master.connect(analyser);
    master.connect(fast);
    master.connect(scope);
    node.port.onmessage = (e: MessageEvent<FromWorklet>) => {
      const m = e.data;
      if (m.type === "telemetry") {
        this.latest = m;
        this.latestAt = performance.now();
        this.latestCreature = null;
        this.load = m.load;
        for (const l of this.listeners) l(m);
      } else if (m.type === "ctelemetry") {
        this.latestCreature = m;
        this.latest = null;
        this.load = m.load;
      } else if (m.type === "planned") {
        this.lastPlan = m;
        for (const l of this.plannedListeners) l(m);
      } else if (m.type === "ready") {
        for (const l of this.readyListeners) l();
      } else if (m.type === "error") {
        this.error = m.message;
        console.error("worklet:", m.message);
      }
    };
    await ctx.resume();
  }

  onTelemetry(l: Listener): () => void {
    this.listeners.add(l);
    return () => this.listeners.delete(l);
  }

  onPlanned(l: (p: PlannedMessage) => void): () => void {
    this.plannedListeners.add(l);
    return () => this.plannedListeners.delete(l);
  }

  /** Send a gestural plan (times relative to now). Returns the request id. */
  schedule(plan: { gestures: import("../wasm/types.ts").Gesture[]; segments: import("../wasm/types.ts").PlannedSegment[]; end: number }, replace = true): number {
    const id = ++this.speakId;
    this.send({ type: "schedule", gestures: plan.gestures, segments: plan.segments, end: plan.end, id, replace });
    return id;
  }

  /** Switch voice ("human" or a creature id), starting the creature from the given settings. */
  setCreature(id: string, params?: Record<string, number>): void {
    // settings still waiting for the next frame belong to the previous creature
    this.pendingCParams = null;
    this.send({ type: "creature", id, params });
  }

  setCreatureParams(values: Record<string, number>): void {
    this.pendingCParams = { ...(this.pendingCParams ?? {}), ...values };
    this.scheduleFlush();
  }

  playCall(id: string, pitch?: number, amp?: number): void {
    this.send({ type: "call", id, pitch, amp });
  }

  stopCall(): void {
    this.send({ type: "stopCall" });
  }

  stopSpeech(): void {
    this.send({ type: "stopSpeech" });
  }

  onReady(l: () => void): () => void {
    this.readyListeners.add(l);
    return () => this.readyListeners.delete(l);
  }

  private send(m: ToWorklet): void {
    this.node?.port.postMessage(m);
  }

  /** Route the voice through the close-mic chain (on) or straight to the output (off). */
  setMic(on: boolean): void {
    if (on === this.micOn) return;
    this.micOn = on;
    if (!this.node || !this.master || !this.micIn) return;
    this.node.disconnect();
    this.node.connect(on ? this.micIn : this.master);
  }

  setBeat(m: { steps?: import("../wasm/types.ts").BeatStep[] | (import("../wasm/types.ts").BeatStep | null)[]; bpm?: number; swing?: number; playing?: boolean }): void {
    this.send({ type: "beat", ...m });
  }

  /** End the sung note (open-ended gestures) with this tag. */
  release(tag: number): void {
    this.send({ type: "release", tag });
  }

  /** Gestures planned on the main thread, times relative to arrival + lead (live playing). */
  scheduleGestures(gestures: import("../wasm/types.ts").Gesture[], lead = 0.006): void {
    this.send({ type: "schedule", gestures, segments: [], end: 0, id: ++this.speakId, replace: false, lead });
  }

  beatHit(id: string, note?: number, accent?: number): void {
    this.send({ type: "beatHit", id, note, accent });
  }

  setVolume(v: number): void {
    if (this.master && this.ctx) this.master.gain.setTargetAtTime(v, this.ctx.currentTime, 0.02);
  }

  setBody(body: BodyMix): void {
    this.send({ type: "body", body });
  }

  gate(level: number): void {
    this.send({ type: "gate", level });
  }

  /** Coalesced: many calls within a frame become one message. */
  setArtic(values: Partial<HumanArtic>, tipFree?: boolean): void {
    this.pendingArtic = { ...(this.pendingArtic ?? {}), ...values };
    if (tipFree !== undefined) this.pendingTipFree = tipFree;
    this.scheduleFlush();
  }

  setSource(values: Partial<VoiceSource>): void {
    this.pendingSource = { ...(this.pendingSource ?? {}), ...values };
    this.scheduleFlush();
  }

  private scheduleFlush(): void {
    if (this.flushScheduled) return;
    this.flushScheduled = true;
    const flush = () => {
      this.flushScheduled = false;
      if (this.pendingArtic || this.pendingTipFree !== undefined) {
        this.send({ type: "artic", values: this.pendingArtic ?? {}, tipFree: this.pendingTipFree });
        this.pendingArtic = null;
        this.pendingTipFree = undefined;
      }
      if (this.pendingSource) {
        this.send({ type: "source", values: this.pendingSource });
        this.pendingSource = null;
      }
      if (this.pendingCParams) {
        this.send({ type: "cparams", values: this.pendingCParams });
        this.pendingCParams = null;
      }
    };
    if (typeof requestAnimationFrame === "function") requestAnimationFrame(flush);
    else setTimeout(flush, 16);
  }
}

export const engine = new AudioEngine();
