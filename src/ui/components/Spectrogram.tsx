import { useRef } from "react";
import { usePhosphor } from "../usePhosphor.ts";
import { engine } from "../../audio/AudioEngine.ts";
import { useStore } from "../store.ts";
import { TEXT, V, mix } from "../colors.ts";
import { beam, label, seg } from "../render/vector.ts";

const FMIN = 60, FMAX = 12000;
const DB_LO = -100, DB_HI = -30;
/** slices kept, and how often one is taken (s): about two seconds of sound */
const SLICES = 28;
const EVERY = 0.07;
/** frequency points per slice (log-spaced) */
const POINTS = 96;

/**
 * Waterfall spectrogram (as on the Fairlight's 3D page): the last couple of seconds as stacked
 * spectral slices, newest in front in white, older ones behind in fading amber, each slice
 * hiding what is behind it.
 */
export function Spectrogram() {
  const st = useRef<{ slices: { t: number; v: Float32Array }[]; bins: Float32Array | null; last: number }>({ slices: [], bins: null, last: -Infinity });
  const ref = usePhosphor(({ ctx, w, h, now }) => {
    const s = st.current;
    const an = engine.analyserFast;
    const started = useStore.getState().started;
    // slices are timestamped, so a slow or hidden tab doesn't stretch "the last two seconds"
    const t = now / 1000, span = SLICES * EVERY;
    if (t - s.last > span) {
      s.slices = [];
      s.last = t - EVERY;
    }
    if (t - s.last >= EVERY) {
      s.last = t;
      const slice = new Float32Array(POINTS);
      if (an && started) {
        const n = an.frequencyBinCount;
        if (!s.bins || s.bins.length !== n) s.bins = new Float32Array(n);
        an.getFloatFrequencyData(s.bins as Float32Array<ArrayBuffer>);
        const binHz = engine.sampleRate / an.fftSize;
        for (let i = 0; i < POINTS; i++) {
          const f0 = FMIN * Math.pow(FMAX / FMIN, i / POINTS), f1 = FMIN * Math.pow(FMAX / FMIN, (i + 1) / POINTS);
          let db = -Infinity;
          for (let k = Math.max(1, Math.floor(f0 / binHz)); k <= Math.min(n - 1, Math.ceil(f1 / binHz)); k++) db = Math.max(db, s.bins[k]);
          slice[i] = Math.max(0, Math.min(1, (db - DB_LO) / (DB_HI - DB_LO)));
        }
      }
      s.slices.push({ t, v: slice });
    }
    while (s.slices.length && t - s.slices[0].t > span) s.slices.shift();
    const left = 26, right = w - 70, base = h - 22;
    const depthX = Math.min(90, (right - left) * 0.18), depthY = Math.min(h * 0.42, 70);
    const frontW = right - left - depthX;
    const amp = Math.max(20, h - 22 - depthY - 12);
    const n = s.slices.length;
    // back to front: fill black under each slice, then stroke it, so nearer slices hide farther ones
    for (let k = 0; k < n; k++) {
      const u = Math.min(1, (t - s.slices[k].t) / span); // 0 = now
      const age = k === n - 1 ? 0 : u * SLICES;
      const ox = left + u * depthX, oy = base - u * depthY;
      const sl = s.slices[k].v;
      const p = new Path2D();
      p.moveTo(ox, oy);
      for (let i = 0; i < POINTS; i++) p.lineTo(ox + (i / (POINTS - 1)) * frontW, oy - Math.pow(sl[i], 1.4) * amp);
      const fill = new Path2D(p);
      fill.lineTo(ox + frontW, oy);
      fill.closePath();
      ctx.save();
      ctx.globalCompositeOperation = "source-over";
      ctx.fillStyle = "#000";
      ctx.fill(fill);
      ctx.restore();
      const fade = 1 - age / SLICES;
      beam(ctx, p, age === 0 ? V.white : mix("#6b4410", V.amber, fade), age === 0 ? 1.4 : 1, { alpha: age === 0 ? 1 : 0.25 + 0.6 * fade, hot: age === 0 });
    }
    // axes: frequency along the front, time going back
    seg(ctx, left, base, left + frontW, base, V.blueDim, 1);
    seg(ctx, left + frontW, base, right, base - depthY, V.blueDim, 1, { dash: [2, 4] });
    for (const [f, t] of [[100, "100"], [1000, "1K"], [10000, "10K"]] as const) {
      const x = left + (Math.log(f / FMIN) / Math.log(FMAX / FMIN)) * frontW;
      seg(ctx, x, base, x, base + 4, V.blueDim, 1);
      label(ctx, t, x, base + 15, { size: 9, color: TEXT.secondary, align: "center", ls: 0.1 });
    }
    label(ctx, "NOW", left + frontW + 6, base + 3, { size: 9, color: TEXT.secondary, ls: 0.14 });
    label(ctx, `${(SLICES * EVERY).toFixed(0)} S AGO`, right + 4, base - depthY + 3, { size: 9, color: TEXT.secondary, ls: 0.14 });
    if (!started) label(ctx, "START AUDIO TO SEE THE SOUND", left, 24, { size: 9, color: TEXT.secondary, ls: 0.16 });
  });
  return <canvas ref={ref} className="instrument-canvas" aria-label="Waterfall spectrogram of the last two seconds of output" role="img" />;
}
