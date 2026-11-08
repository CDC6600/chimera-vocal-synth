import { useRef } from "react";
import { usePhosphor } from "../usePhosphor.ts";
import { useStore } from "../store.ts";
import { creatureById } from "../../wasm/engine.ts";
import type { CreatureDef } from "../../wasm/types.ts";
import { liveTelemetry, previewTelemetry } from "../creatureLive.ts";
import { TEXT, V, SLOTS, heat, pressureLevel } from "../colors.ts";
import { beamPts, dot, hfont, label, labelWidth, ring, seg, wrapLabel, type Pt } from "../render/vector.ts";

type P = [number, number];

/** Catmull-Rom through the points; closed shapes wrap around. */
function smooth(pts: P[], per: number, closed: boolean): P[] {
  const n = pts.length;
  if (n < 3) return pts.slice();
  const at = (i: number) => (closed ? pts[(i + n) % n] : pts[Math.max(0, Math.min(n - 1, i))]);
  const out: P[] = [];
  const segs = closed ? n : n - 1;
  for (let i = 0; i < segs; i++) {
    const p0 = at(i - 1), p1 = at(i), p2 = at(i + 1), p3 = at(i + 2);
    for (let k = 0; k < per; k++) {
      const t = k / per, t2 = t * t, t3 = t2 * t;
      const f = (a: number, b: number, c: number, d: number) => 0.5 * (2 * b + (-a + c) * t + (2 * a - 5 * b + 4 * c - d) * t2 + (-a + 3 * b - 3 * c + d) * t3);
      out.push([f(p0[0], p1[0], p2[0], p3[0]), f(p0[1], p1[1], p2[1], p3[1])]);
    }
  }
  if (!closed) out.push(pts[n - 1]);
  return out;
}

interface TubeGeom {
  tube: string;
  label?: string;
  inView: boolean;
  pts: P[];
  cum: number[];
  length: number;
}

interface Geom {
  silhouette: P[];
  parts: { shade: number; pts: P[] }[];
  tubes: TubeGeom[];
}

const geoms = new Map<string, Geom>();
function geometry(def: CreatureDef): Geom {
  let g = geoms.get(def.id);
  if (g) return g;
  const v = def.visual;
  g = {
    silhouette: smooth(v.silhouette, 8, true),
    parts: (v.parts ?? []).map((p) => ({ shade: p.shade, pts: smooth(p.pts, 6, true) })),
    tubes: v.tubes.map((t) => {
      const pts = smooth(t.path, 6, false);
      const cum = [0];
      for (let i = 1; i < pts.length; i++) cum.push(cum[i - 1] + Math.hypot(pts[i][0] - pts[i - 1][0], pts[i][1] - pts[i - 1][1]));
      return { tube: t.tube, label: t.label, inView: t.inView !== false, pts, cum, length: cum[cum.length - 1] };
    }),
  };
  geoms.set(def.id, g);
  return g;
}

function pointAt(t: TubeGeom, s: number): P {
  let i = 1;
  while (i < t.cum.length - 1 && t.cum[i] < s) i++;
  const u = Math.max(0, Math.min(1, (s - t.cum[i - 1]) / Math.max(1e-9, t.cum[i] - t.cum[i - 1])));
  return [t.pts[i - 1][0] + (t.pts[i][0] - t.pts[i - 1][0]) * u, t.pts[i - 1][1] + (t.pts[i][1] - t.pts[i - 1][1]) * u];
}

/** text cut to fit a width (measured as label() draws it), ending in an ellipsis */
function fit(ctx: CanvasRenderingContext2D, text: string, maxW: number, size: number, ls: number): string {
  let t = text;
  while (t.length > 2 && labelWidth(ctx, t, { size, ls }) > maxW) t = t.slice(0, -2) + "…";
  return t;
}

/** a creature parameter's value for display */
function fmtParam(v: number, p: CreatureDef["params"][number]): { value: string; unit: string } {
  const digits = p.digits ?? (p.max - p.min <= 2 ? 2 : v < 100 && p.log ? 1 : 0);
  return { value: v.toFixed(digits), unit: p.unit ?? "" };
}

/**
 * The creature as a vector drawing, in the language of the human face: the outline in bright blue,
 * tissue dimmer, airway walls at the simulated width with cross-section rungs lit by sound
 * pressure, sound sources as rings in slot colours with ripples paced by the pitch, and the first
 * four parameters as readouts across the bottom.
 */
export function CreatureView() {
  const glyph = useRef<{ peak: Record<string, number>; phase: Record<string, number> }>({ peak: {}, phase: {} });
  const ref = usePhosphor(({ ctx, w, h, dt }) => {
    const st = useStore.getState();
    const def = creatureById(st.creature);
    if (!def) return;
    const vis = def.visual;
    const geo = geometry(def);
    const live = liveTelemetry(def.id);
    const tele = live ?? previewTelemetry(def, st.cparams[def.id]);
    // a narrow screen (a phone) stacks the title and status, and puts the readouts two by two
    const compact = w < 560;
    const top = compact ? 96 : 64, slotH = compact ? 132 : Math.min(118, h * 0.2);
    const vw = vis.view.x1 - vis.view.x0, vh = vis.view.y1 - vis.view.y0;
    const sc = Math.min((w * 0.86) / vw, ((h - top - slotH) * 0.9) / vh);
    const ox = w / 2 - ((vis.view.x0 + vis.view.x1) / 2) * sc;
    const oy = top + (h - top - slotH) / 2 + ((vis.view.y0 + vis.view.y1) / 2) * sc;
    const S = ([x, y]: P): Pt => [ox + x * sc, oy - y * sc];

    beamPts(ctx, geo.silhouette.map(S), V.blue, 1.7, { hot: true, close: true });
    for (const part of geo.parts) beamPts(ctx, part.pts.map(S), part.shade > 0.5 ? V.blueMid : V.blueDim, 1, { close: true, alpha: 0.9 });

    // airways: walls at the simulated width (drawn wider than life so narrow tubes stay legible)
    const exaggerate = vis.tubeScale ?? 1.6;
    const labels: { text: string; x: number; y: number; color: string }[] = [];
    for (const tg of geo.tubes) {
      const data = tele.tubes[tg.tube];
      if (!data) continue;
      const n = data.areas.length;
      const half = (k: number) => Math.sqrt(Math.max(0, data.areas[Math.min(n - 1, k)]) / Math.PI) * exaggerate;
      const frame = (s: number) => {
        const a = pointAt(tg, Math.max(0, s - 0.02 * tg.length)), b = pointAt(tg, Math.min(tg.length, s + 0.02 * tg.length));
        const dx = b[0] - a[0], dy = b[1] - a[1], l = Math.hypot(dx, dy) || 1;
        return { p: pointAt(tg, s), nx: -dy / l, ny: dx / l };
      };
      const wallA: Pt[] = [], wallB: Pt[] = [];
      for (let k = 0; k <= n; k++) {
        const { p, nx, ny } = frame((k / n) * tg.length);
        const r = half(k);
        wallA.push(S([p[0] + nx * r, p[1] + ny * r]));
        wallB.push(S([p[0] - nx * r, p[1] - ny * r]));
      }
      beamPts(ctx, wallA, V.blue, 1.2, { hot: true });
      beamPts(ctx, wallB, V.blue, 1.2, { hot: true });
      for (let k = 0; k < n; k++) {
        const { p, nx, ny } = frame(((k + 0.5) / n) * tg.length);
        const r = half(k);
        const lvl = live && st.view.pressure ? Math.pow(pressureLevel(data.rms[k] * 0.6), 0.85) : 0;
        const [x0, y0] = S([p[0] + nx * r, p[1] + ny * r]), [x1, y1] = S([p[0] - nx * r, p[1] - ny * r]);
        seg(ctx, x0, y0, x1, y1, live ? heat(lvl) : V.blueDim, 1, { alpha: live ? 0.35 + 0.65 * lvl : 0.6 });
      }
      if (tg.label && tg.inView) {
        const m = S(pointAt(tg, tg.length * 0.5));
        labels.push({ text: tg.label.toUpperCase(), x: m[0] + 12, y: m[1] - 12, color: TEXT.secondary });
      }
    }

    // sound sources: a ring sized by the vibration envelope, ripples paced by the pitch
    const pp = def.params.find((p) => p.role === "pitch");
    const paramValue = (key: string, dflt: number) => (live ? (live.params[key] ?? dflt) : (st.cparams[def.id]?.[key] ?? dflt));
    const pitch = pp ? paramValue(pp.key, pp.default) : 200;
    const rate = 1.2 + 4.8 * Math.max(0, Math.min(1, Math.log(pitch / 10) / Math.log(900)));
    (vis.sources ?? []).forEach((s, i) => {
      const color = SLOTS[i % 4];
      const raw = Math.max(0, Math.min(1, live?.source[s.key] ?? 0));
      const pk = Math.max(raw, (glyph.current.peak[s.key] ?? 0) * Math.exp(-dt / 0.18));
      glyph.current.peak[s.key] = pk;
      const ph = ((glyph.current.phase[s.key] ?? 0) + dt * rate) % 1;
      glyph.current.phase[s.key] = ph;
      const [x, y] = S(s.at);
      const r0 = 3.5 + 6 * pk;
      if (pk > 0.04) for (let j = 0; j < 3; j++) {
        const u = (ph + j / 3) % 1;
        ring(ctx, x, y, r0 + u * 24, color, 1, { alpha: (1 - u) * Math.min(1, pk * 1.3) * 0.8 });
      }
      ring(ctx, x, y, r0, color, 1.5, { hot: true });
      dot(ctx, x, y, 1.8, color);
      labels.push({ text: s.label.toUpperCase(), x: x + 14, y: y + 18, color });
    });

    if (st.view.calipers) {
      // place labels, nudging down past any earlier label they would overlap
      ctx.font = hfont(10);
      const placed: { x0: number; x1: number; y0: number; y1: number }[] = [];
      for (const l of labels) {
        const tw = ctx.measureText(l.text).width + l.text.length * 1.5;
        const x = Math.min(w - tw - 6, l.x);
        let y = l.y;
        for (let guard = 0; guard < 8; guard++) {
          const hit = placed.find((r) => x < r.x1 + 4 && x + tw > r.x0 - 4 && y - 11 < r.y1 && y + 2 > r.y0);
          if (!hit) break;
          y = hit.y1 + 12;
        }
        placed.push({ x0: x, x1: x + tw, y0: y - 11, y1: y + 2 });
        label(ctx, l.text, x, y, { size: 10, color: l.color, ls: 0.15 });
      }
    }

    // like a map's title: the name, the latin name, and what it is doing
    label(ctx, fit(ctx, def.name.toUpperCase(), w - 32, compact ? 18 : 24, 0.18), 16, compact ? 30 : 34, { size: compact ? 18 : 24, color: TEXT.primary, ls: 0.18 });
    label(ctx, fit(ctx, def.latin.toUpperCase(), w - 34, 10, 0.16), 18, compact ? 48 : 54, { size: 10, color: TEXT.secondary, ls: 0.16 });
    const state = live ? (live.call ? (def.calls.find((c) => c.id === live.call)?.name ?? live.call) : live.gate > 0.05 ? "Sustaining" : "At rest") : "Not started";
    const pitchText = pp ? (() => { const f = fmtParam(pitch, pp); return `${f.value} ${f.unit}`.trim().toUpperCase(); })() : "";
    if (compact) {
      label(ctx, `${state.toUpperCase()}${pitchText ? "   " + pitchText : ""}`, 18, 70, { size: 11, color: V.amber, ls: 0.14 });
    } else {
      label(ctx, state.toUpperCase(), w - 16, 30, { size: 13, color: V.amber, align: "right", ls: 0.16 });
      if (pitchText) label(ctx, pitchText, w - 16, 50, { size: 11, color: V.amber, align: "right", ls: 0.12, alpha: 0.8 });
    }
    // a scale bar: 1 drawing unit (about a centimetre)
    const unit = vw > 60 ? 10 : 1;
    const by = h - slotH - 10;
    seg(ctx, 18, by, 18 + unit * sc, by, V.blueMid, 1.2);
    seg(ctx, 18, by - 4, 18, by + 4, V.blueMid, 1.2);
    seg(ctx, 18 + unit * sc, by - 4, 18 + unit * sc, by + 4, V.blueMid, 1.2);
    label(ctx, `${unit} CM`, 26 + unit * sc, by + 3, { size: 9, color: TEXT.secondary, ls: 0.14 });

    // the first four parameters, OP-1 style, across the bottom (the encoders on the right share the colours)
    const params = def.params.slice(0, 4);
    const perRow = compact ? 2 : Math.max(1, params.length);
    const cw = (w - 28) / perRow, rowH = compact ? slotH / 2 : slotH;
    const big = Math.max(18, Math.min(40, rowH * (compact ? 0.42 : 0.36), cw * 0.2));
    params.forEach((p, i) => {
      const color = SLOTS[i];
      const cx = 16 + (i % perRow) * cw, cy = h - slotH + 12 + Math.floor(i / perRow) * rowH;
      dot(ctx, cx + 5, cy, 4, color);
      // a long name takes a second line above, where the first row of slots has room, before an ellipsis
      const lo = { size: 10, color, ls: 0.16, alpha: 0.85 };
      const lines = wrapLabel(ctx, p.label.toUpperCase(), cw - 22, lo);
      if (lines.length > 1 && i < perRow) {
        label(ctx, lines[0], cx + 15, cy - 9, lo);
        label(ctx, fit(ctx, lines.slice(1).join(" "), cw - 22, 10, 0.16), cx + 15, cy + 4, lo);
      } else label(ctx, fit(ctx, p.label.toUpperCase(), cw - 22, 10, 0.16), cx + 15, cy + 4, lo);
      const f = fmtParam(paramValue(p.key, p.default), p);
      label(ctx, f.value, cx, cy + 14 + big, { size: big, color, weight: "m", ls: 0.02 });
      // under the value, or beside it on a phone, where two rows of slots leave no room below
      if (f.unit && compact) label(ctx, f.unit.toUpperCase(), cx + labelWidth(ctx, f.value, { size: big, weight: "m", ls: 0.02 }) + 6, cy + 14 + big, { size: 10, color, ls: 0.14, alpha: 0.7 });
      else if (f.unit) label(ctx, f.unit.toUpperCase(), cx, cy + 32 + big, { size: 10, color, ls: 0.14, alpha: 0.7 });
    });
  });
  return <canvas ref={ref} className="creature-canvas" aria-label="Creature airway: its outline, its airways lit by sound pressure, and its sound sources" role="img" />;
}
