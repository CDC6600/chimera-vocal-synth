import { useRef } from "react";
import { usePhosphor } from "../usePhosphor.ts";
import { engine } from "../../audio/AudioEngine.ts";
import { sameBody } from "../body.ts";
import { useStore } from "../store.ts";
import { useModel } from "../useModel.ts";
import { VoiceModel } from "../analysis.ts";
import { TEXT, V } from "../colors.ts";
import { beam, beamPts, label, seg, type Pt } from "../render/vector.ts";

const FMIN = 60, FMAX = 8000;
const RANGE = 62; // dB shown

/**
 * The live output as harmonic stems (from the analyser, at multiples of the sung f0), the vocal
 * tract's transfer function over them, formant markers, and the pitch in the pitch colour.
 * Pass `ring` to mark the singer's-ring band.
 */
export function SpectrumView(props: { ring?: boolean }) {
  const model = useModel();
  const bins = useRef<Float32Array | null>(null);
  const ringRef = useRef(props.ring);
  ringRef.current = props.ring;
  const ref = usePhosphor(({ ctx, w, h }) => {
    const st = useStore.getState();
    const m = model.current;
    if (!m) return;
    const left = 36, right = w - 14, top = 22, bot = h - 22;
    const fx = (f: number) => left + ((Math.log(f) - Math.log(FMIN)) / (Math.log(FMAX) - Math.log(FMIN))) * (right - left);
    const dbY = (db: number) => top + (Math.min(0, Math.max(-RANGE, db)) / -RANGE) * (bot - top);
    const tele = engine.latest && sameBody(engine.latest.body, st.body) ? engine.latest : null;
    const f0 = tele && tele.t.voicing > 0.3 ? tele.t.f0 : 0;

    for (const f of [100, 200, 500, 1000, 2000, 5000]) {
      seg(ctx, fx(f), top, fx(f), bot, V.frame, 1);
      if (!(f0 && Math.abs(fx(f) - fx(f0)) < 40)) label(ctx, f >= 1000 ? `${f / 1000}K` : String(f), fx(f), h - 6, { size: 9, color: TEXT.secondary, align: "center", ls: 0.1 });
    }
    for (const db of [-20, -40]) {
      seg(ctx, left, dbY(db), right, dbY(db), V.frame, 1, { dash: [2, 4] });
      label(ctx, String(db), left - 6, dbY(db) + 3, { size: 8, color: TEXT.secondary, align: "right", ls: 0.05 });
    }
    seg(ctx, left, bot, right, bot, V.blueDim, 1);
    if (ringRef.current) {
      const p = new Path2D();
      p.rect(fx(2400), top - 6, fx(3600) - fx(2400), bot - top + 6);
      beam(ctx, p, V.white, 1, { dash: [3, 4], alpha: 0.6 });
      label(ctx, "RING", fx(3600) + 4, top + 4, { size: 9, color: V.white, ls: 0.16 });
    }

    // the output, from the analyser: stems at the harmonics while voiced, else the whole curve
    const an = engine.analyser;
    if (an && st.started) {
      if (!bins.current || bins.current.length !== an.frequencyBinCount) bins.current = new Float32Array(an.frequencyBinCount);
      an.getFloatFrequencyData(bins.current as Float32Array<ArrayBuffer>);
      const b = bins.current;
      const sr = engine.sampleRate;
      const binOf = (f: number) => Math.min(b.length - 1, Math.max(0, Math.round((f / sr) * 2 * b.length)));
      const peak = (f: number, half: number) => {
        let mx = -Infinity;
        for (let k = binOf(f - half); k <= binOf(f + half); k++) mx = Math.max(mx, b[k]);
        return mx;
      };
      if (f0 > 0) {
        const hs: Pt[] = [];
        for (let k = 1; k * f0 < FMAX; k++) if (k * f0 >= FMIN) hs.push([k * f0, peak(k * f0, f0 * 0.3)]);
        const mx = Math.max(...hs.map((q) => q[1]));
        const stems = new Path2D();
        for (const [f, db] of hs) {
          stems.moveTo(fx(f), bot);
          stems.lineTo(fx(f), dbY(db - mx));
        }
        beam(ctx, stems, V.amber, 1.1, { alpha: 0.55 });
      } else {
        const pts: Pt[] = [];
        let mx = -Infinity;
        for (let x = left; x <= right; x += 2) {
          const f = Math.exp(Math.log(FMIN) + ((x - left) / (right - left)) * (Math.log(FMAX) - Math.log(FMIN)));
          const db = b[binOf(f)];
          mx = Math.max(mx, db);
          pts.push([x, db]);
        }
        beamPts(ctx, pts.map(([x, db]) => [x, dbY(db - mx)] as Pt), V.amber, 1, { alpha: 0.45 });
      }
    }

    // the transfer function of the tract, with its formants
    const a = tele ? VoiceModel.articFromArray(tele.t.artic) : st.artic;
    const tf = m.transfer(a);
    let mx = -Infinity;
    for (const d of tf.db) mx = Math.max(mx, d);
    const env: Pt[] = [];
    for (let i = 0; i < tf.freqs.length; i++) if (tf.freqs[i] >= FMIN && tf.freqs[i] <= FMAX) env.push([fx(tf.freqs[i]), dbY(tf.db[i] - mx)]);
    beamPts(ctx, env, V.amber, 1.5, { hot: true });
    tf.formants.slice(0, 3).forEach((F, i) => {
      let near = 0;
      for (let k = 0; k < tf.freqs.length; k++) if (Math.abs(tf.freqs[k] - F) < Math.abs(tf.freqs[near] - F)) near = k;
      const x = fx(F), y = Math.max(top + 12, dbY(tf.db[near] - mx) - 8);
      const tri = new Path2D();
      tri.moveTo(x - 5, y - 8);
      tri.lineTo(x + 5, y - 8);
      tri.lineTo(x, y);
      tri.closePath();
      beam(ctx, tri, V.amber, 1.2);
      label(ctx, `F${i + 1}`, x, y - 12, { size: 10, color: V.amber, align: "center", ls: 0.1 });
    });
    if (f0 > 0) {
      const x = fx(f0);
      const tri = new Path2D();
      tri.moveTo(x - 5, bot + 9);
      tri.lineTo(x + 5, bot + 9);
      tri.lineTo(x, bot + 1);
      tri.closePath();
      beam(ctx, tri, V.green, 1.2);
      label(ctx, `F0 ${Math.round(f0)}`, x - 9, bot + 13, { size: 9, color: V.green, align: "right", ls: 0.12 });
    }
  });
  return <canvas ref={ref} className="instrument-canvas" aria-label="Output spectrum: harmonics, the vocal tract's resonances and its formants" role="img" />;
}
