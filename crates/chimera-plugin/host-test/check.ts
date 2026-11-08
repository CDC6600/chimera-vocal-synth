/**
 * Judge what host_test.lua rendered in a real host: did each format load, do notes and the sequencer
 * sound when they should, on the right pitch, and on the grid once the host has compensated for the
 * reported delay? Prints every number, so a failure says how far off it was.
 *
 *   bun crates/chimera-plugin/host-test/check.ts <folder>
 */
import { existsSync, readFileSync } from "node:fs";

/** A hit's start may be off by this much (s). */
const ON_GRID = 0.02;
/** A sung note may be off by this much (cents). */
const IN_TUNE = 50;
/** What the host should hear reported in Beatbox (s): BEAT_LATENCY in crates/chimera-plugin/src/core.rs. */
const BEAT_LATENCY = 0.18;
/** A host may report it at its device's rate rather than the rendered one (Hz). */
const RATES = [44100, 48000];
/**
 * Two renders of one case are the same sound when their loudness over time differs by less than this
 * on average (dB). Not their samples: the voice runs on while it's silent, with the sound device
 * open, so it meets the first note in a different state each time. That difference is inaudible.
 */
const SAME_SOUND = 6;
/** How far macos/host-test.sh drags the grip inwards (points), and what the window may differ by. */
const GRIP_DRAG = [100, 60], GRIP_SLACK = 3;
/** REAPER's title bar and toolbar above the editor (points): CHROME in macos/host-test.sh. */
const REAPER_CHROME = 54;
/** Above this a stretch is sounding, and below SILENT it isn't. */
const SOUNDING = 1e-3, SILENT = 1e-4;

function readWav(path: string): { fs: number; x: Float64Array } {
  const b = readFileSync(path);
  const view = new DataView(b.buffer, b.byteOffset, b.byteLength);
  let at = 12, format = 0, channels = 0, fs = 0, bits = 0;
  while (at + 8 <= b.length) {
    const id = b.toString("latin1", at, at + 4);
    const size = view.getUint32(at + 4, true);
    if (id === "fmt ") {
      format = view.getUint16(at + 8, true);
      channels = view.getUint16(at + 10, true);
      fs = view.getUint32(at + 12, true);
      bits = view.getUint16(at + 22, true);
      if (format === 0xfffe) format = view.getUint16(at + 32, true);
    } else if (id === "data") {
      const width = bits / 8, frames = Math.floor(size / width / channels);
      const x = new Float64Array(frames);
      for (let i = 0; i < frames; i++) {
        let sum = 0;
        for (let c = 0; c < channels; c++) {
          const p = at + 8 + (i * channels + c) * width;
          if (format === 3) sum += bits === 32 ? view.getFloat32(p, true) : view.getFloat64(p, true);
          else if (bits === 16) sum += view.getInt16(p, true) / 32768;
          else if (bits === 24) sum += ((view.getInt8(p + 2) << 16) | (view.getUint8(p + 1) << 8) | view.getUint8(p)) / 8388608;
          else sum += view.getInt32(p, true) / 2147483648;
        }
        x[i] = sum / channels;
      }
      return { fs, x };
    }
    at += 8 + size + (size & 1);
  }
  throw new Error(`${path}: no audio in it`);
}

type Wav = ReturnType<typeof readWav>;

function rms({ fs, x }: Wav, from: number, to: number): number {
  const a = Math.max(0, Math.round(from * fs)), b = Math.min(x.length, Math.round(to * fs));
  let sum = 0;
  for (let i = a; i < b; i++) sum += x[i] * x[i];
  return Math.sqrt(sum / Math.max(1, b - a));
}

/** When the sound starting near `t` starts: the first sample above a tenth of the peak nearby. */
function onset({ fs, x }: Wav, t: number): { at: number; peak: number } {
  const a = Math.max(0, Math.round((t - 0.1) * fs)), b = Math.min(x.length, Math.round((t + 0.2) * fs));
  let peak = 0;
  for (let i = a; i < b; i++) peak = Math.max(peak, Math.abs(x[i]));
  for (let i = a; i < b; i++) if (Math.abs(x[i]) >= peak / 10) return { at: i / fs, peak };
  return { at: NaN, peak };
}

/** The pitch in a stretch by autocorrelation, between 60 and 600 Hz. */
function pitch({ fs, x }: Wav, from: number, to: number): number {
  const a = Math.round(from * fs), n = Math.round((to - from) * fs);
  const lo = Math.round(fs / 600), hi = Math.round(fs / 60);
  const corr = (lag: number) => {
    let c = 0;
    for (let i = 0; i < n; i++) c += x[a + i] * x[a + i + lag];
    return c;
  };
  const zero = corr(0);
  let best = lo, top = -Infinity;
  for (let lag = lo; lag <= hi; lag++) {
    const c = corr(lag);
    // the shortest strong lag: a multiple of the period correlates as well
    if (c > top * 1.02 && c > 0.3 * zero) {
      top = c;
      best = lag;
    }
  }
  const [l, m, r] = [corr(best - 1), corr(best), corr(best + 1)];
  return fs / (best + (0.5 * (l - r)) / (l - 2 * m + r));
}

/** Where a file sounds, as from-to in seconds, for a failure to show. */
function spans(w: Wav): string {
  const step = 0.01, out: string[] = [];
  let from = -1;
  for (let t = 0; t < w.x.length / w.fs; t += step) {
    const on = rms(w, t, t + step) > SOUNDING;
    if (on && from < 0) from = t;
    if (!on && from >= 0) {
      out.push(`${from.toFixed(2)}-${t.toFixed(2)}`);
      from = -1;
    }
  }
  if (from >= 0) out.push(`${from.toFixed(2)}-end`);
  return out.join(" ");
}

/** The average difference in level, over stretches where either file is sounding (dB). */
function loudnessDifference(a: Wav, b: Wav): number {
  const step = 0.02, seconds = Math.min(a.x.length / a.fs, b.x.length / b.fs), level = (r: number) => 20 * Math.log10(r + 1e-5);
  let sum = 0, n = 0;
  for (let t = 0; t + step <= seconds; t += step) {
    const [ra, rb] = [rms(a, t, t + step), rms(b, t, t + step)];
    if (ra < SOUNDING && rb < SOUNDING) continue;
    sum += Math.abs(level(ra) - level(rb));
    n++;
  }
  return n ? sum / n : 0;
}

const folder = process.argv[2];
if (!folder) throw new Error("which folder?");

let failures = 0;
function check(name: string, ok: boolean, detail: string): void {
  if (!ok) failures++;
  console.log(`${ok ? "ok  " : "FAIL"} ${name}: ${detail}`);
}

const records = readFileSync(`${folder}/results.jsonl`, "utf8").trim().split("\n").map((l) => JSON.parse(l) as Record<string, string | number>);
const midiHz = (m: number) => 440 * Math.pow(2, (m - 69) / 12);

const firstOfCase: Record<string, { format: string; wav: Wav }> = {};

const judge: Record<string, (name: string, w: Wav) => void> = {
  sing(name, w) {
    check(`${name} silent before the first note`, rms(w, 0, 0.45) < SILENT, `rms ${rms(w, 0, 0.45).toExponential(1)}`);
    for (const [t, m] of [[0.5, 48], [2, 52], [3.5, 55], [5, 60]]) {
      const level = rms(w, t, t + 1);
      check(`${name} sounds at ${t} s`, level > SOUNDING, `rms ${level.toExponential(1)}`);
      const hz = pitch(w, t + 0.45, t + 0.85), cents = 1200 * Math.log2(hz / midiHz(m));
      check(`${name} midi ${m} in tune`, Math.abs(cents) < IN_TUNE, `${hz.toFixed(1)} Hz, ${cents.toFixed(0)} cents off`);
    }
  },
  beatbox(name, w) {
    const errors: number[] = [], peaks: number[] = [];
    for (let beat = 0; beat < 16; beat++) {
      const t = beat * 0.5, { at, peak } = onset(w, t);
      peaks.push(peak);
      if (peak < SOUNDING) check(`${name} kick on beat ${beat}`, false, `peak ${peak.toExponential(1)}`);
      else errors.push(at - t);
    }
    const ms = errors.map((e) => (e * 1000).toFixed(1)).join(" ");
    check(`${name} every kick on the grid`, errors.length === 16 && errors.every((e) => Math.abs(e) < ON_GRID), `ms off: ${ms}`);
    // a kick's peak varies with its exact timing, but the first shouldn't be lost to the host's start
    const middle = [...peaks].sort((a, b) => a - b)[8];
    check(`${name} first kick as strong as the rest`, peaks[0] > middle / 3, `peaks: ${peaks.map((p) => p.toFixed(2)).join(" ")}`);
  },
  pads(name, w) {
    const errors: number[] = [];
    for (const t of [1, 2, 3, 4]) {
      const { at, peak } = onset(w, t);
      if (peak < SOUNDING) check(`${name} pad at ${t} s`, false, `peak ${peak.toExponential(1)}`);
      else errors.push(at - t);
    }
    const ms = errors.map((e) => (e * 1000).toFixed(1)).join(" ");
    check(`${name} every pad on its note`, errors.length === 4 && errors.every((e) => Math.abs(e) < ON_GRID), `ms off: ${ms}`);
    check(`${name} no pattern before the first note`, rms(w, 0, 0.8) < SILENT, `rms ${rms(w, 0, 0.8).toExponential(1)}`);
  },
  // the example pattern, after a click on the Beatbox tab: kick or snare on every other step
  clicked(name, w) {
    judge.beatbox(name, w);
  },
  speak(name, w) {
    check(`${name} silent before the first note`, rms(w, 0, 0.45) < SILENT, `rms ${rms(w, 0, 0.45).toExponential(1)}`);
    for (const [t, line] of [[0.5, 1], [3.5, 2]]) {
      const level = rms(w, t, t + 2);
      check(`${name} says line ${line}`, level > SOUNDING, `rms ${level.toExponential(1)}`);
    }
  },
};

for (const r of records) {
  if (r.reaper) {
    console.log(`REAPER ${r.reaper} on ${r.os}`);
    continue;
  }
  if (!r.format) {
    check("the script ran", false, String(r.error));
    continue;
  }
  const name = `${r.format} ${r.case}`;
  if (r.case === "editor") {
    check(`${name} click reached the plugin`, r.mode === "beatbox", `mode ${r.mode}, reported delay ${r.pdc}`);
    // the window as the driver saw it: the saved size alone can't tell a refusal from a resize
    const boundsFile = `${folder}/bounds-${r.format}.txt`;
    if (!existsSync(boundsFile)) {
      check(`${name} grip resized the window`, false, "the driver never got to the plugin's view");
      continue;
    }
    const [before, after] = readFileSync(boundsFile, "utf8").trim().split("\n").map((l) => l.trim().split(" ").map(Number));
    const narrower = before[2] - after[2], lower = before[3] - after[3];
    check(`${name} grip resized the window`, Math.abs(narrower - GRIP_DRAG[0]) <= GRIP_SLACK && Math.abs(lower - GRIP_DRAG[1]) <= GRIP_SLACK, `${before[2]}x${before[3]} to ${after[2]}x${after[3]}`);
    const [w, h] = String(r.window).split(",").map(Number);
    check(`${name} the project keeps that size`, w === after[2] && h === after[3] - REAPER_CHROME, `saved ${r.window}`);
    continue;
  }
  if (r.error) {
    check(name, false, String(r.error));
    continue;
  }
  if (r.skipped) {
    console.log(`skip ${name}: ${r.skipped}`);
    continue;
  }
  const beat = r.case === "beatbox" || r.case === "pads" || r.case === "clicked";
  const expected = beat ? RATES.map((fs) => BEAT_LATENCY * fs) : [0];
  check(`${name} reports its delay`, expected.some((e) => Math.abs(Number(r.pdc) - e) <= 1), `${r.pdc} samples, loaded as "${r.fx_name}"`);
  const file = `${folder}/${r.format}-${r.case}.wav`;
  if (!existsSync(file)) {
    check(`${name} rendered`, false, "no file");
    continue;
  }
  const wav = readWav(file);
  judge[r.case as string](name, wav);
  // every format is the same binary, so it should render the same sound
  const first = firstOfCase[r.case as string] ??= { format: String(r.format), wav };
  if (first.wav !== wav) {
    const db = loudnessDifference(first.wav, wav);
    const same = db < SAME_SOUND;
    check(`${name} sounds like ${first.format}`, same, `loudness differs by ${db.toFixed(1)} dB`);
    if (!same) {
      console.log(`     ${first.format}: ${spans(first.wav)}`);
      console.log(`     ${r.format}: ${spans(wav)}`);
    }
  }
}

console.log(failures ? `\n${failures} failed` : "\nall passed");
process.exit(failures ? 1 : 0);
