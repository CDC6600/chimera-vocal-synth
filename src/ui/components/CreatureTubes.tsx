import { usePhosphor } from "../usePhosphor.ts";
import { useStore } from "../store.ts";
import { creatureById } from "../../wasm/engine.ts";
import { liveTelemetry, previewTelemetry } from "../creatureLive.ts";
import { TEXT, V, heat, pressureLevel } from "../colors.ts";
import { beam, beamPts, label, type Pt } from "../render/vector.ts";

const a2d = (A: number) => 2 * Math.sqrt(Math.max(0, A) / Math.PI);

/** Each airway tube unrolled into a lane: one ring per section (shared diameter scale), lit by sound pressure. */
export function CreatureTubes() {
  const ref = usePhosphor(({ ctx, w, h }) => {
    const st = useStore.getState();
    const def = creatureById(st.creature);
    if (!def) return;
    const live = liveTelemetry(def.id);
    const tele = live ?? previewTelemetry(def, st.cparams[def.id]);
    const lanes = def.visual.tubes.filter((t) => tele.tubes[t.tube]);
    if (!lanes.length) return;
    let dMax = 0.01;
    for (const l of lanes) for (const a of tele.tubes[l.tube].areas) dMax = Math.max(dMax, a2d(a));
    const labelW = 120;
    const laneH = (h - 6) / lanes.length;
    lanes.forEach((l, li) => {
      const data = tele.tubes[l.tube];
      const y = 3 + laneH * (li + 0.5);
      label(ctx, (l.label ?? l.tube).toUpperCase(), 14, y + 3, { size: 9, color: TEXT.secondary, ls: 0.16 });
      const n = data.areas.length;
      const x0 = labelW, x1 = w - 14;
      const dx = (x1 - x0) / n;
      const r = (k: number) => Math.max(0.8, (a2d(data.areas[k]) / dMax) * (laneH - 6) * 0.5);
      const topL: Pt[] = [], botL: Pt[] = [];
      for (let k = 0; k < n; k++) {
        const x = x0 + (k + 0.5) * dx;
        topL.push([x, y - r(k)]);
        botL.push([x, y + r(k)]);
        const lv = live ? Math.pow(pressureLevel(data.rms[k] * 0.6), 0.85) : 0;
        const p = new Path2D();
        p.ellipse(x, y, Math.max(1, r(k) * 0.28), r(k), 0, 0, Math.PI * 2);
        beam(ctx, p, live ? heat(lv) : V.blueDim, 1, { alpha: live ? 0.35 + 0.65 * lv : 0.6 });
      }
      beamPts(ctx, topL, V.blue, 1.1);
      beamPts(ctx, botL, V.blue, 1.1);
    });
  });
  return <canvas ref={ref} className="instrument-canvas" aria-label="Airway tubes unrolled, lit by sound pressure" role="img" />;
}
