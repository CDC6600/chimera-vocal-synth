import { useEffect, useRef } from "react";
import { usePhosphor } from "../usePhosphor.ts";
import { engine } from "../../audio/AudioEngine.ts";
import { currentPlan, onPlan, type SpokenPlan } from "../speech.ts";
import { activation, type Gesture } from "../../wasm/engine.ts";
import { TEXT, V } from "../colors.ts";
import { beam, beamPts, blockLineH, label, labelBlock, seg, wrapLabel, type Pt } from "../render/vector.ts";

/** rows of the score, coloured like the articulators on the face (lips pink, tongue green, soft palate white, glottis amber) */
const ROWS: { name: string; keys: string[]; color: string }[] = [
  { name: "Lips", keys: ["lipAp", "lipProt", "lipWidth", "labiodental"], color: V.pink },
  { name: "Tongue tip", keys: ["ttX", "ttY", "ttCurl", "lateral"], color: V.green },
  { name: "Tongue body", keys: ["tbX", "tbY", "root"], color: V.green },
  { name: "Soft palate", keys: ["velum"], color: V.white },
  { name: "Glottis", keys: ["abduction", "closure"], color: V.amber },
];

function rowOf(g: Gesture): number {
  const keys = Object.keys(g.targets);
  // vowel gestures touch body + lips; draw them on the body row
  if (keys.includes("tbX") && keys.includes("lipAp") && keys.length > 4) return 2;
  // tip-driven segments that also round the lips (/ɹ ʃ/) belong to the tongue-tip row
  if (keys.includes("ttX") || keys.includes("ttCurl")) return 1;
  for (let r = 0; r < ROWS.length; r++) if (keys.some((k) => ROWS[r].keys.includes(k))) return r;
  return -1;
}

/** current time on the plan's clock (s), or -1 when nothing is playing */
export function planNow(sp: SpokenPlan | null): number {
  const tele = engine.latest;
  return sp && sp.offset !== null && tele ? tele.t.clock - sp.offset : -1;
}

/** Gestural score of the current utterance (Articulatory Phonology), with a live playhead. */
export function ScoreView() {
  const planRef = useRef<SpokenPlan | null>(currentPlan());
  useEffect(() => onPlan((p) => (planRef.current = p)), []);
  const ref = usePhosphor(({ ctx, w, h }) => {
    const sp = planRef.current;
    const left = 104, right = w - 14, top = 18, bottom = h - 26;
    const rowH = (bottom - top) / (ROWS.length + 1);
    ROWS.forEach((r, i) => label(ctx, r.name.toUpperCase(), 14, top + rowH * (i + 0.62), { size: 9, color: r.color, ls: 0.16, alpha: 0.85 }));
    label(ctx, "PITCH", 14, top + rowH * (ROWS.length + 0.62), { size: 9, color: V.amber, ls: 0.16, alpha: 0.85 });
    for (let i = 1; i <= ROWS.length + 1; i++) seg(ctx, left, top + rowH * i, right, top + rowH * i, V.frame, 1);
    if (!sp) {
      // on black over the rules, wrapped to the score's width
      const msg = "SPEAK SOMETHING TO SEE ITS GESTURES: WHICH ARTICULATOR MOVES, WHEN, AND HOW THEY OVERLAP.";
      const cy = (top + bottom) / 2;
      const bh = wrapLabel(ctx, msg, right - left - 16, { size: 10, ls: 0.12 }).length * blockLineH(10) + 8;
      ctx.fillStyle = "#000";
      ctx.fillRect(left, cy - bh / 2, right - left, bh);
      labelBlock(ctx, msg, left + 8, cy, right - left - 16, { size: 10, color: TEXT.secondary, ls: 0.12 });
      return;
    }
    const plan = sp.plan;
    const T0 = -0.05, T1 = plan.end + 0.15;
    const X = (t: number) => left + ((t - T0) / (T1 - T0)) * (right - left);
    const now = planNow(sp);
    const f0s: { t: number; f: number }[] = [];
    for (const g of plan.gestures) {
      if (g.targets.f0 !== undefined) {
        f0s.push({ t: (g.t0 + g.t1) / 2, f: g.targets.f0 });
        continue;
      }
      const r = rowOf(g);
      if (r < 0) continue;
      const yb = top + rowH * (r + 1) - 3, yt = top + rowH * r + 7;
      const x0 = X(g.t0 - g.rampIn), xa = X(g.t0), xb = X(g.t1), x1 = X(g.t1 + g.rampOut);
      const isVowel = r === 2 && Object.keys(g.targets).length > 4;
      const act = now >= g.t0 - g.rampIn && now <= g.t1 + g.rampOut ? activation(g, now) : 0;
      // a gesture is a trapezoid: ramp in, plateau, ramp out
      const p = new Path2D();
      p.moveTo(x0, yb);
      p.lineTo(xa, yt);
      p.lineTo(xb, yt);
      p.lineTo(x1, yb);
      beam(ctx, p, ROWS[r].color, act > 0.05 ? 1.5 : 1.1, { alpha: act > 0.05 ? 0.6 + 0.4 * act : isVowel ? 0.42 : 0.75, hot: act > 0.5 });
      if (g.label && xb - xa > 10) {
        const lbl = g.label === "aspiration" || g.label === "voiceless" ? "OPEN" : g.label === "velum" ? "LOWERED" : g.label;
        label(ctx, lbl, (xa + xb) / 2, yb - 4, { size: 10, color: ROWS[r].color, align: "center", ls: 0.08, alpha: isVowel && act < 0.05 ? 0.6 : 1 });
      }
    }
    // the pitch contour
    if (f0s.length) {
      f0s.sort((a, b) => a.t - b.t);
      const fmin = Math.min(...f0s.map((q) => q.f)) * 0.92, fmax = Math.max(...f0s.map((q) => q.f)) * 1.08;
      const yb = top + rowH * (ROWS.length + 1) - 4, ya = top + rowH * ROWS.length + 6;
      const pts: Pt[] = f0s.map((q) => [X(q.t), yb - ((q.f - fmin) / Math.max(1, fmax - fmin)) * (yb - ya)]);
      beamPts(ctx, pts, V.amber, 1.3);
      const lastP = pts[pts.length - 1];
      label(ctx, `${Math.round(f0s[f0s.length - 1].f)} HZ`, lastP[0] + 6, lastP[1] + 3, { size: 9, color: V.amber, ls: 0.1 });
    }
    // segments along the bottom
    let lastRight = -Infinity;
    for (const s of plan.segments) {
      const x = X((s.t0 + s.t1) / 2);
      if (x - 6 < lastRight) continue;
      lastRight = x + 6;
      const on = now >= s.t0 && now < s.t1;
      seg(ctx, X(s.t0), bottom + 2, X(s.t0), bottom + 6, V.blueDim, 1);
      label(ctx, s.ipa, x, h - 8, { size: 12, color: on ? V.white : TEXT.primary, align: "center", ls: 0 });
    }
    // playhead
    if (now >= T0 && now <= T1) {
      const x = X(now);
      seg(ctx, x, top - 4, x, bottom, V.white, 1.2, { hot: true });
      const tri = new Path2D();
      tri.moveTo(x - 5, top - 10);
      tri.lineTo(x + 5, top - 10);
      tri.lineTo(x, top - 3);
      tri.closePath();
      beam(ctx, tri, V.white, 1.1);
      label(ctx, `${Math.max(0, now).toFixed(2)} S`, x + 8, top - 4, { size: 9, color: V.white, ls: 0.12, alpha: 0.8 });
    }
  });
  return <canvas ref={ref} className="score-canvas" aria-label="Gestural score of the current utterance: which articulator moves, when, and how they overlap" role="img" />;
}
