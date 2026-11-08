import { useRef } from "react";
import { usePhosphor } from "../usePhosphor.ts";
import { engine } from "../../audio/AudioEngine.ts";
import { useStore } from "../store.ts";
import { blendLog, sameBody, type BodyMix } from "../body.ts";
import { useModel } from "../useModel.ts";
import { VoiceModel } from "../analysis.ts";
import { TEXT, V } from "../colors.ts";
import { beamPts, handle, ipaGlyph, label, seg, type Pt } from "../render/vector.ts";
import { nearestVowel } from "./AnatomyView.tsx";

/**
 * Vowel map in the phonetician's orientation: F2 decreasing left→right, F1 increasing downward.
 * Calibrated vowels are plotted where this body actually produces them; dragging morphs the
 * articulation between them; the green ring is the live tract's measured (F1, F2), with a trail.
 */
export function VowelMap() {
  const model = useModel();
  const dragging = useRef(false);
  const range = () => {
    const b = useStore.getState().body;
    const at = (male: number, female: number, child: number) => blendLog(b, { male, female, child });
    return { f1: [at(230, 280, 300), at(950, 1150, 1250)], f2: [at(700, 800, 900), at(2700, 3200, 3500)] };
  };
  const toXY = (F1: number, F2: number, w: number, h: number) => {
    const r = range();
    const pad = 18;
    const x = pad + ((Math.log(r.f2[1]) - Math.log(F2)) / (Math.log(r.f2[1]) - Math.log(r.f2[0]))) * (w - 2 * pad);
    const y = pad + ((Math.log(F1) - Math.log(r.f1[0])) / (Math.log(r.f1[1]) - Math.log(r.f1[0]))) * (h - 2 * pad);
    return { x, y };
  };
  const fromXY = (x: number, y: number, w: number, h: number) => {
    const r = range();
    const pad = 18;
    const u = Math.min(1, Math.max(0, (x - pad) / (w - 2 * pad)));
    const v = Math.min(1, Math.max(0, (y - pad) / (h - 2 * pad)));
    const F2 = Math.exp(Math.log(r.f2[1]) - u * (Math.log(r.f2[1]) - Math.log(r.f2[0])));
    const F1 = Math.exp(Math.log(r.f1[0]) + v * (Math.log(r.f1[1]) - Math.log(r.f1[0])));
    return { F1, F2 };
  };
  /** recent (F1, F2) of the live tract, so the trail survives resizes and fades out */
  const trail = useRef<{ F1: number; F2: number; t: number; body: BodyMix }[]>([]);
  const ref = usePhosphor(({ ctx, w, h }) => {
    const m = model.current;
    if (!m) return;
    const st = useStore.getState();
    // vowel trapezoid guide, like a border on a map
    const corners = [toXY(300, 2500, w, h), toXY(320, 850, w, h), toXY(720, 1050, w, h), toXY(780, 1650, w, h)];
    beamPts(ctx, corners.map((p) => [p.x, p.y] as Pt), V.blueDim, 1, { close: true, alpha: 0.9 });
    const tele = engine.latest && sameBody(engine.latest.body, st.body) ? engine.latest : null;
    const a = tele ? VoiceModel.articFromArray(tele.t.artic) : st.artic;
    const fm = m.transfer(a).formants;
    const live = fm.length >= 2 ? toXY(fm[0], fm[1], w, h) : null;
    const near = fm.length >= 2 ? nearestVowel(m, fm[0], fm[1]) : "";
    for (const v of m.vowels) {
      if (v.ipa === near && live) continue;
      const p = toXY(v.F1, v.F2, w, h);
      ipaGlyph(ctx, p.x, p.y + 5, v.ipa, 15, TEXT.secondary);
    }
    if (live) {
      // where the tongue has been: a trail of the last 1.5 s, fading with age
      const now = performance.now();
      const tr = (trail.current = trail.current.filter((p) => sameBody(p.body, st.body) && now - p.t < 1500));
      const last = tr[tr.length - 1];
      if (!last || Math.abs(Math.log(last.F1 / fm[0])) + Math.abs(Math.log(last.F2 / fm[1])) > 0.004) tr.push({ F1: fm[0], F2: fm[1], t: now, body: st.body });
      for (let i = 1; i < tr.length; i++) {
        const p0 = toXY(tr[i - 1].F1, tr[i - 1].F2, w, h), p1 = toXY(tr[i].F1, tr[i].F2, w, h);
        seg(ctx, p0.x, p0.y, p1.x, p1.y, V.green, 1.2, { alpha: 0.7 * (1 - (now - tr[i].t) / 1500) });
      }
      handle(ctx, live.x, live.y, V.green, 8);
      ipaGlyph(ctx, live.x + 20, live.y - 8, near, 17, V.green);
    }
    label(ctx, "F2", w - 6, h - 5, { size: 9, color: TEXT.secondary, align: "right", ls: 0.1 });
    label(ctx, "F1", 4, h - 5, { size: 9, color: TEXT.secondary, ls: 0.1 });
  });

  const apply = (e: React.PointerEvent) => {
    const m = model.current;
    if (!m) return;
    const r = (e.currentTarget as HTMLCanvasElement).getBoundingClientRect();
    const { F1, F2 } = fromXY(e.clientX - r.left, e.clientY - r.top, r.width, r.height);
    useStore.getState().setArtic(m.articForFormants(F1, F2), true);
  };

  return (
    <canvas
      ref={ref}
      className="vowel-map"
      onPointerDown={(e) => {
        dragging.current = true;
        (e.target as Element).setPointerCapture(e.pointerId);
        apply(e);
      }}
      onPointerMove={(e) => dragging.current && apply(e)}
      onPointerUp={() => (dragging.current = false)}
      onPointerCancel={() => (dragging.current = false)}
      aria-label="Vowel map. Drag to morph between vowels."
      role="slider"
      aria-valuetext="vowel position"
    />
  );
}
