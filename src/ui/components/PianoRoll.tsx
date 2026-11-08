import { useRef } from "react";
import { usePhosphor } from "../usePhosphor.ts";
import { engine } from "../../audio/AudioEngine.ts";
import { useStore } from "../store.ts";
import { roll, tickSinging } from "../singing.ts";
import { TEXT, V } from "../colors.ts";
import { beam, beamPts, label, seg, type Pt } from "../render/vector.ts";
import { nearestBody } from "../body.ts";

/** pixels per second of voice time */
const SPEED = 96;
/** the "now" line, as a fraction of the width: the rest shows what is coming */
const NOW_AT = 0.7;
/** semitones shown */
const SPAN = 25;
const BLACK = [1, 3, 6, 8, 10];
const NAMES = ["C", "C♯", "D", "D♯", "E", "F", "F♯", "G", "G♯", "A", "A♯", "B"];
const KEY_CENTRE: Record<string, number> = { male: 54, female: 66, child: 70 };

const hzToMidi = (f: number) => 69 + 12 * Math.log2(f / 440);

/**
 * Scrolling piano roll for Sing mode: the notes asked for (green frames, with their syllables)
 * and the pitch the vocal folds actually produced (an amber trace with the onsets, glides and
 * vibrato), on the vector display.
 */
export function PianoRoll() {
  const st = useRef({ centre: NaN, trace: [] as { t: number; m: number; v: number }[], lastClock: -1 });
  const ref = usePhosphor(({ ctx, w, h, dt }) => {
    tickSinging();
    const s = st.current;
    const body = useStore.getState().body;
    const tele = engine.latest?.t;
    const clock = engine.voiceClock() ?? 0;
    // sung pitch, sampled as telemetry arrives
    if (tele && tele.clock !== s.lastClock) {
      s.lastClock = tele.clock;
      s.trace.push({ t: tele.clock, m: tele.f0 > 20 ? hzToMidi(tele.f0) : NaN, v: tele.voicing });
      const cut = tele.clock - (w * NOW_AT) / SPEED - 1;
      while (s.trace.length && s.trace[0].t < cut) s.trace.shift();
    }
    const left = 34;
    const tLeft = clock - ((w - left) * NOW_AT) / SPEED, tRight = clock + ((w - left) * (1 - NOW_AT)) / SPEED;
    const x = (t: number) => left + (t - tLeft) * SPEED;
    // follow the notes on screen (slowly), else rest on the keyboard's middle
    let lo = Infinity, hi = -Infinity;
    for (const n of roll.notes) {
      if ((n.t1 ?? clock) < tLeft || n.t0 > tRight) continue;
      lo = Math.min(lo, n.midi);
      hi = Math.max(hi, n.midi);
    }
    const want = lo <= hi ? (lo + hi) / 2 : KEY_CENTRE[nearestBody(body)];
    if (!Number.isFinite(s.centre)) s.centre = want;
    s.centre += (want - s.centre) * (1 - Math.exp(-dt / 0.6));
    const bottom = s.centre - SPAN / 2;
    const top = 6;
    const rowH = (h - top - 4) / SPAN;
    const y = (m: number) => h - 4 - (m - bottom + 0.5) * rowH;

    // rows: a line at each C (named), dotted lines through the black keys
    const dotted = new Path2D();
    for (let m = Math.floor(bottom) - 1; m <= Math.ceil(bottom + SPAN) + 1; m++) {
      const pc = ((m % 12) + 12) % 12;
      const yc = y(m);
      if (yc < top || yc > h) continue;
      if (BLACK.includes(pc)) {
        dotted.moveTo(left, yc);
        dotted.lineTo(w, yc);
      }
      if (pc === 0) {
        seg(ctx, left, yc + rowH / 2, w, yc + rowH / 2, V.frame, 1);
        label(ctx, `${NAMES[pc]}${Math.floor(m / 12) - 1}`, 6, yc + 3, { size: 9, color: TEXT.secondary, ls: 0.1 });
      }
    }
    beam(ctx, dotted, V.frame, 1, { dash: [1, 5] });

    // notes
    const barH = Math.max(4, rowH - 3);
    for (const n of roll.notes) {
      const t1 = n.t1 ?? clock;
      if (t1 < tLeft || n.t0 > tRight) continue;
      const x0 = Math.max(left, x(n.t0)), x1 = Math.max(x0 + 3, x(t1));
      const yc = y(n.midi);
      const live = n.t0 <= clock && clock <= t1;
      const future = n.t0 > clock;
      const r = new Path2D();
      r.rect(x0, yc - barH / 2, x1 - x0, barH);
      if (live) {
        ctx.save();
        ctx.globalCompositeOperation = "lighter";
        ctx.fillStyle = "rgba(91, 227, 129, 0.12)";
        ctx.fill(r);
        ctx.restore();
      }
      beam(ctx, r, V.green, live ? 1.6 : 1.2, { alpha: live ? 1 : future ? 0.38 : 0.62, hot: live });
      if (n.text && n.text !== "~") label(ctx, n.text.toUpperCase(), x0 + 1, yc - barH / 2 - 4, { size: 10, color: live ? V.white : TEXT.secondary, ls: 0.12, alpha: future ? 0.6 : 1 });
    }

    // the pitch actually sung
    let run: Pt[] = [];
    const flush = () => {
      if (run.length > 1) beamPts(ctx, run, V.amber, 1.5, { hot: true });
      run = [];
    };
    for (const p of s.trace) {
      if (!(p.v > 0.3) || !Number.isFinite(p.m) || p.t < tLeft) {
        flush();
        continue;
      }
      run.push([x(p.t), y(p.m)]);
    }
    flush();

    // now
    const xn = left + (w - left) * NOW_AT;
    seg(ctx, xn, top, xn, h, V.white, 1, { dash: [3, 4], alpha: 0.6 });
  });
  return <canvas ref={ref} className="instrument-canvas roll-canvas" aria-label="Piano roll: the notes played, their syllables, and the pitch actually sung" />;
}
