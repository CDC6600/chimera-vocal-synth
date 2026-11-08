import { usePhosphor } from "../usePhosphor.ts";
import { engine } from "../../audio/AudioEngine.ts";
import { sameBody } from "../body.ts";
import { useStore } from "../store.ts";
import { useModel } from "../useModel.ts";
import { VoiceModel } from "../analysis.ts";
import { TEXT, V, heat, pressureLevel } from "../colors.ts";
import { beam, beamPts, label, seg, type Pt } from "../render/vector.ts";

/**
 * The simulated tube as a wireframe: one ring per section (diameter from the area function),
 * lit by acoustic pressure, with the nasal branch above the velum junction.
 */
export function TubeView() {
  const model = useModel();
  const ref = usePhosphor(({ ctx, w, h }) => {
    const st = useStore.getState();
    const m = model.current;
    if (!m) return;
    const tele = engine.latest && sameBody(engine.latest.body, st.body) ? engine.latest : null;
    let ph: ArrayLike<number>, or: ArrayLike<number>, phLen: number, orLen: number;
    let rms: ArrayLike<number> | null = null;
    if (tele) {
      ph = tele.t.phAreas;
      or = tele.t.orAreas;
      phLen = tele.t.phLen;
      orLen = tele.t.orLen;
      rms = tele.t.pressureRms;
    } else {
      const ar = m.areas(st.artic);
      ph = ar.phAreas;
      or = ar.orAreas;
      phLen = ar.phLen;
      orLen = ar.orLen;
    }
    const nPh = ph.length, nOr = or.length;
    const total = phLen + orLen;
    const x0 = 16, x1 = w - 16;
    const sx = (x1 - x0) / total;
    const axisY = h * 0.6;
    const dS = (h * 0.3) / 4; // 4 cm diameter → 30% of the height
    const a2d = (A: number) => 2 * Math.sqrt(Math.max(0, A) / Math.PI);
    const secs: { x: number; r: number; lv: number }[] = [];
    for (let i = 0; i < nPh; i++) secs.push({ x: x0 + ((i + 0.5) / nPh) * phLen * sx, r: (a2d(ph[i]) * dS) / 2, lv: rms ? pressureLevel(rms[i]) : 0 });
    for (let i = 0; i < nOr; i++) secs.push({ x: x0 + (phLen + ((i + 0.5) / nOr) * orLen) * sx, r: (a2d(or[i]) * dS) / 2, lv: rms ? pressureLevel(rms[nPh + i]) : 0 });

    // nasal branch: dashed and dim while the velum is shut
    const velum = tele ? tele.t.velumPort : m.frame(st.artic).velumPort;
    const open = velum > 0.02;
    const jx = x0 + phLen * sx;
    const nTop = axisY - h * 0.36;
    const nLen = Math.min(x1 - jx - 40, 11.4 * m.bodyParams.scale * sx * 0.62);
    const nc = open ? V.white : V.blueDim;
    const dash = open ? undefined : [3, 4];
    for (const off of [-5, 5]) {
      const p = new Path2D();
      p.moveTo(jx + off, axisY - 26);
      p.lineTo(jx + off, nTop + off + 2);
      p.quadraticCurveTo(jx + off, nTop + off, jx + 8, nTop + off);
      p.lineTo(jx + nLen, nTop + off);
      beam(ctx, p, nc, 1, { dash });
    }
    label(ctx, "NOSE", jx + nLen + 6, nTop + 3, { size: 9, color: TEXT.secondary, ls: 0.16 });
    label(ctx, open ? `VELUM OPEN ${velum.toFixed(2)} CM²` : "VELUM SHUT", jx, nTop - 12, { size: 9, color: TEXT.secondary, ls: 0.16, align: "center" });

    // silhouette and rings
    const top: Pt[] = [[x0, axisY - 3], ...secs.map((q) => [q.x, axisY - q.r] as Pt), [x1, axisY - secs[secs.length - 1].r]];
    const bot: Pt[] = [[x0, axisY + 3], ...secs.map((q) => [q.x, axisY + q.r] as Pt), [x1, axisY + secs[secs.length - 1].r]];
    beamPts(ctx, top, V.blue, 1.3, { hot: true });
    beamPts(ctx, bot, V.blue, 1.3, { hot: true });
    for (const q of secs) {
      const lv = Math.pow(q.lv, 0.85);
      const p = new Path2D();
      p.ellipse(q.x, axisY, Math.max(1.2, q.r * 0.28), Math.max(0.6, q.r), 0, 0, Math.PI * 2);
      beam(ctx, p, rms ? heat(lv) : V.blueDim, 1.1, { alpha: rms ? 0.35 + 0.65 * lv : 0.6 });
    }
    seg(ctx, x0, axisY, x1, axisY, V.blueDim, 0.8, { dash: [2, 5], alpha: 0.8 });

    // landmarks
    const f = m.frame(tele ? VoiceModel.articFromArray(tele.t.artic) : st.artic);
    const marks: [number, string][] = [[0, "GLOTTIS"], [phLen, "VELUM"], [f.teethS, "TEETH"], [total, "LIPS"]];
    let lastRight = -Infinity;
    for (const [s, name] of marks) {
      const x = x0 + s * sx;
      const tw = name.length * 9 * 0.78;
      const lx = Math.min(w - tw - 2, Math.max(2, x - tw / 2));
      if (lx < lastRight + 8) continue;
      lastRight = lx + tw;
      seg(ctx, x, h - 26, x, h - 21, V.blueDim, 1);
      label(ctx, name, lx, h - 9, { size: 9, color: TEXT.secondary, ls: 0.16 });
    }
  });
  return <canvas ref={ref} className="instrument-canvas" aria-label="Area function of the simulated tube, lit by sound pressure" role="img" />;
}
