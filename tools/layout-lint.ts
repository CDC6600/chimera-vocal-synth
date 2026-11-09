/**
 * Layout lint: a smoke test of the layout. Loads every mode at phone, tablet and desktop sizes in
 * headless Chromium, before and after audio starts, then some alternate states (every encoder page,
 * every creature, the 3D face, a phrase being spoken) and the duet page (`?show=`, with a short
 * duet it renders first), and fails on mistakes screenshot reviews kept missing:
 *
 *  - overlap     two pieces of text or controls drawn over each other (DOM text, buttons, inputs,
 *                dials, and text drawn on canvases, which the page reports by wrapping fillText)
 *  - overflow    text wider than its own box, running into whatever is beside it
 *  - clipped     content cut off by an overflow: hidden box (a panel too short for what it holds)
 *  - spill       content that runs outside its screen's frame
 *  - screens     two screens, or a screen and the top bar, overlapping
 *  - canvas-edge canvas text running off its canvas
 *  - h-scroll    the page scrolls sideways
 *  - small-text  DOM text under 8 px or canvas text under 7 px
 *
 * Opt-outs, for things meant to sit on top of other things:
 *  - data-layout="overlay" on an element: it and its contents are exempt from overlap checks
 *    (the start prompt and the beat letter float over the face on purpose)
 *  - data-layout="layered" on an element: its contents may overlap each other, but not anything
 *    outside it (piano keys: the black keys sit on the white ones)
 *  - data-layout="scatter" on a canvas: its text may overlap other text on the same canvas
 *    (a chart that plots labels at data positions)
 *  - label(..., { moving: true }) on canvas text in flight (a word flying off the lips)
 *  Text truncated with an ellipsis or a line clamp is reported as a warning, not an error.
 *
 * Usage: bun tools/layout-lint.ts [--url http://...] [--out out/layout] [--only 1440x900] [--mode play|duet]
 * Without --url it starts its own Vite dev server (build the wasm first: bun run wasm).
 * Screenshots of every failing view are saved under --out.
 */
import { chromium } from "playwright-core";
import { mkdirSync, rmSync, writeFileSync } from "node:fs";
import { basename } from "node:path";

const arg = (name: string) => {
  const i = process.argv.indexOf(`--${name}`);
  return i > 0 ? process.argv[i + 1] : undefined;
};

export const VIEWPORTS: { w: number; h: number }[] = [
  { w: 375, h: 812 }, // phone
  { w: 768, h: 1024 }, // tablet, portrait
  { w: 1024, h: 768 }, // tablet, landscape; small laptop
  { w: 1280, h: 720 },
  { w: 1366, h: 768 },
  { w: 1440, h: 900 },
  { w: 1920, h: 1080 },
  { w: 2560, h: 1440 },
];
const MODES = ["play", "speak", "sing", "bestiary", "beatbox"] as const;

interface Problem {
  kind: string;
  detail: string;
  warn?: boolean;
}

/** runs in the page: collects the boxes and returns the problems */
function inspect(): Problem[] {
  type R = { x0: number; y0: number; x1: number; y1: number };
  type Atom = { el: Element; r: R; name: string; overlay: boolean; canvas?: HTMLCanvasElement; scatter?: boolean };
  const TOL = 1.5;
  const problems: Problem[] = [];
  const rect = (b: DOMRect): R => ({ x0: b.left, y0: b.top, x1: b.right, y1: b.bottom });
  const meet = (a: R, b: R): R => ({ x0: Math.max(a.x0, b.x0), y0: Math.max(a.y0, b.y0), x1: Math.min(a.x1, b.x1), y1: Math.min(a.y1, b.y1) });
  const w = (r: R) => r.x1 - r.x0;
  const h = (r: R) => r.y1 - r.y0;
  const empty = (r: R) => w(r) <= 0.5 || h(r) <= 0.5;
  const fmt = (r: R) => `[${Math.round(r.x0)},${Math.round(r.y0)} ${Math.round(w(r))}×${Math.round(h(r))}]`;
  const where = (el: Element): string => {
    const screen = el.closest(".screen")?.getAttribute("aria-label") ?? (el.closest(".topbar") ? "top bar" : "page");
    return screen;
  };
  const nameOf = (el: Element, text?: string): string => {
    const t = (text ?? (el as HTMLElement).innerText ?? el.getAttribute("aria-label") ?? "").replace(/\s+/g, " ").trim().slice(0, 40);
    const cls = typeof el.className === "string" && el.className ? "." + el.className.trim().split(/\s+/)[0] : "";
    return `${el.tagName.toLowerCase()}${cls}${t ? ` "${t}"` : ""} in ${where(el)}`;
  };
  const visible = (el: Element) => el.checkVisibility({ opacityProperty: true, visibilityProperty: true });
  const overlay = (el: Element) => !!el.closest('[data-layout="overlay"]');

  // the part of a box its overflow-clipping ancestors let through; `hidden` is what overflow:hidden cut off
  const clipOf = (el: Element, r: R, self: boolean) => {
    let vis = r;
    let cutBy: Element | null = null;
    let truncated = false;
    for (let a = self ? el : el.parentElement; a && a !== document.documentElement; a = a.parentElement) {
      const cs = getComputedStyle(a);
      if (cs.textOverflow === "ellipsis" || (cs.webkitLineClamp && cs.webkitLineClamp !== "none")) truncated = true;
      if (cs.overflowX === "visible" && cs.overflowY === "visible") continue;
      const before = vis;
      vis = meet(vis, rect(a.getBoundingClientRect()));
      const scrolls = /auto|scroll/.test(cs.overflowX + cs.overflowY);
      if (!scrolls && !cutBy && (w(before) - w(vis) > TOL || h(before) - h(vis) > TOL)) cutBy = a;
    }
    return { vis, cutBy, truncated };
  };

  const atoms: Atom[] = [];
  // text is clipped by its own element too; a control's own box is not
  const consider = (el: Element, r: R, name: string, extra: Partial<Atom> = {}, self = false) => {
    if (empty(r)) return;
    const { vis, cutBy, truncated } = clipOf(el, r, self);
    if (cutBy) problems.push({ kind: truncated ? "truncated" : "clipped", warn: truncated, detail: `${name} ${fmt(r)} is cut off by ${nameOf(cutBy, "")}` });
    if (empty(vis)) return; // scrolled out of view
    atoms.push({ el, r: vis, name, overlay: overlay(el), ...extra });
  };

  // DOM text, one box per line of each text node
  const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
  for (let n = walker.nextNode(); n; n = walker.nextNode()) {
    const text = n.textContent ?? "";
    const el = n.parentElement;
    if (!el || !text.trim() || !visible(el) || el.closest("script,style,option")) continue;
    if (el.closest("textarea")) continue; // the textarea itself is the box
    const size = parseFloat(getComputedStyle(el).fontSize);
    if (size < 8) problems.push({ kind: "small-text", detail: `${nameOf(el, text)} is ${size}px` });
    const range = document.createRange();
    range.selectNodeContents(n);
    const own = rect(el.getBoundingClientRect());
    const cs = getComputedStyle(el);
    for (const b of range.getClientRects()) {
      const r = rect(b);
      consider(el, r, nameOf(el, text), {}, true);
      // text wider than its own box (a table heading over its neighbour's) where nothing clips it
      if (cs.display !== "inline" && cs.overflowX === "visible" && (own.x0 - r.x0 > TOL || r.x1 - own.x1 > TOL))
        problems.push({ kind: "overflow", detail: `${nameOf(el, text)} ${fmt(r)} is wider than its box ${fmt(own)}` });
    }
  }
  // controls and drawings without text of their own
  for (const el of document.querySelectorAll("button, input, textarea, select, svg, img")) {
    if (el.closest("svg") !== el && el.tagName.toLowerCase() === "svg") continue;
    if (el.parentElement?.closest("svg")) continue;
    if (!visible(el)) continue;
    consider(el, rect(el.getBoundingClientRect()), nameOf(el));
  }
  // canvas text, recorded by the fillText hook installed before the page loaded
  const log = (window as unknown as { __canvasText?: Map<HTMLCanvasElement, { text: string; x0: number; y0: number; x1: number; y1: number; size: number }[]> }).__canvasText;
  if (log) {
    for (const [canvas, items] of log) {
      if (!canvas.isConnected || !visible(canvas)) continue;
      const cb = canvas.getBoundingClientRect();
      if (cb.width < 2 || cb.height < 2) continue;
      const sx = cb.width / canvas.width, sy = cb.height / canvas.height;
      const scatter = canvas.closest('[data-layout="scatter"]') !== null;
      for (const t of items) {
        const r: R = { x0: cb.left + t.x0 * sx, y0: cb.top + t.y0 * sy, x1: cb.left + t.x1 * sx, y1: cb.top + t.y1 * sy };
        const name = `canvas text "${t.text.slice(0, 40)}" in ${where(canvas)}`;
        if (t.size * sy < 7) problems.push({ kind: "small-text", detail: `${name} is ${(t.size * sy).toFixed(1)}px` });
        const inside = meet(r, rect(cb));
        if (w(r) - w(inside) > 2 || h(r) - h(inside) > 2) problems.push({ kind: "canvas-edge", detail: `${name} ${fmt(r)} runs off its canvas ${fmt(rect(cb))}` });
        consider(canvas, inside, name, { canvas, scatter });
      }
    }
  }

  // overlaps between unrelated atoms
  for (let i = 0; i < atoms.length; i++) {
    for (let j = i + 1; j < atoms.length; j++) {
      const a = atoms[i], b = atoms[j];
      if (a.overlay || b.overlay) continue;
      if (a.el === b.el && !a.canvas) continue;
      if (a.canvas && a.canvas === b.canvas && (a.scatter || b.scatter)) continue;
      if (a.el !== b.el && (a.el.contains(b.el) || b.el.contains(a.el))) continue;
      const layer = a.el.closest('[data-layout="layered"]');
      if (layer && layer === b.el.closest('[data-layout="layered"]')) continue;
      const m = meet(a.r, b.r);
      if (w(m) > TOL && h(m) > TOL) problems.push({ kind: "overlap", detail: `${a.name} ${fmt(a.r)} overlaps ${b.name} ${fmt(b.r)}` });
    }
  }

  // content outside its screen's frame
  for (const a of atoms) {
    const screen = a.el.closest(".screen");
    if (!screen || a.overlay) continue;
    const s = rect(screen.getBoundingClientRect());
    const m = meet(a.r, s);
    if (w(a.r) - w(m) > TOL || h(a.r) - h(m) > TOL) problems.push({ kind: "spill", detail: `${a.name} ${fmt(a.r)} runs outside its screen ${fmt(s)}` });
  }

  // a screen's parts inside its frame (a body taller than the screen pokes out of it)
  for (const screen of document.querySelectorAll(".screen")) {
    if (!visible(screen)) continue;
    const s = rect(screen.getBoundingClientRect());
    for (const part of screen.children) {
      const r = rect(part.getBoundingClientRect());
      if (r.x1 - s.x1 > TOL || r.y1 - s.y1 > TOL || s.x0 - r.x0 > TOL || s.y0 - r.y0 > TOL) problems.push({ kind: "spill", detail: `${nameOf(part, "")} ${fmt(r)} pokes out of its screen ${fmt(s)}` });
    }
  }

  // screens against each other and the top bar
  const frames = [...document.querySelectorAll(".screen, .topbar")].filter(visible);
  for (let i = 0; i < frames.length; i++) {
    for (let j = i + 1; j < frames.length; j++) {
      const m = meet(rect(frames[i].getBoundingClientRect()), rect(frames[j].getBoundingClientRect()));
      if (w(m) > TOL && h(m) > TOL) problems.push({ kind: "screens", detail: `${nameOf(frames[i], "")} overlaps ${nameOf(frames[j], "")}` });
    }
  }

  const sw = document.documentElement.scrollWidth;
  if (sw > window.innerWidth + 1) problems.push({ kind: "h-scroll", detail: `the page is ${sw}px wide in a ${window.innerWidth}px window` });
  return problems;
}

/**
 * Before any page script: record the text each canvas has drawn since it was last wiped. Every
 * screen starts its frame by filling (or clearing) the whole canvas, so a wipe starts a new list;
 * text from earlier frames, or erased since, is never inspected.
 */
function hookCanvasText() {
  type Item = { text: string; x0: number; y0: number; x1: number; y1: number; size: number };
  const log = new Map<HTMLCanvasElement, Item[]>();
  (window as unknown as { __canvasText: typeof log }).__canvasText = log;
  const P = CanvasRenderingContext2D.prototype;
  for (const name of ["fillRect", "clearRect"] as const) {
    const orig = P[name];
    P[name] = function (this: CanvasRenderingContext2D, x: number, y: number, w: number, h: number) {
      orig.call(this, x, y, w, h);
      const t = this.getTransform(), c = this.canvas;
      const x0 = Math.min(t.a * x, t.a * (x + w)) + t.e, y0 = Math.min(t.d * y, t.d * (y + h)) + t.f;
      const x1 = Math.max(t.a * x, t.a * (x + w)) + t.e, y1 = Math.max(t.d * y, t.d * (y + h)) + t.f;
      if (x0 <= 0 && y0 <= 0 && x1 >= c.width && y1 >= c.height) log.set(c as HTMLCanvasElement, []);
    };
  }
  const fill = P.fillText;
  P.fillText = function (this: CanvasRenderingContext2D, text: string, x: number, y: number, maxWidth?: number) {
    fill.call(this, text, x, y, maxWidth as number);
    // label({ moving: true }) marks text in flight, meant to cross other text (src/ui/render/vector.ts)
    if (!text.trim() || this.globalAlpha < 0.05 || (this as { layoutMoving?: boolean }).layoutMoving) return;
    const m = this.measureText(text);
    const t = this.getTransform();
    // the box in canvas pixels (the transforms used here only scale and translate)
    const xs = [x - m.actualBoundingBoxLeft, x + m.actualBoundingBoxRight].map((v) => t.a * v + t.e);
    const ys = [y - m.actualBoundingBoxAscent, y + m.actualBoundingBoxDescent].map((v) => t.d * v + t.f);
    const size = parseFloat(/(\d+(?:\.\d+)?)px/.exec(this.font)?.[1] ?? "10") * Math.abs(t.d);
    const c = this.canvas as HTMLCanvasElement;
    if (!log.has(c)) log.set(c, []);
    log.get(c)!.push({ text, x0: Math.min(...xs), y0: Math.min(...ys), x1: Math.max(...xs), y1: Math.max(...ys), size });
  };
}

/** A two-voice show for the duet page, rendered by the CLI now (shows are never kept in git). The
 *  lint serves it at shows/lint-duet/ itself, so it needs no files under public/. */
function makeDuet(dir: string): void {
  mkdirSync(dir, { recursive: true });
  const melody = (notes: number[][]) => JSON.stringify({ bpm: 120, t0: 0, lyrics: "la la la la", notes });
  writeFileSync(`${dir}/low.json`, melody([[0, 1], [55, 1], [57, 1], [59, 1], [60, 2]]));
  writeFileSync(`${dir}/high.json`, melody([[0, 1], [64, 1], [65, 1], [67, 1], [67, 2]]));
  for (const [v, body] of [["low", "male"], ["high", "female"]]) {
    const r = Bun.spawnSync(["cargo", "run", "--quiet", "--release", "-p", "chimera-cli", "--", "render", "notes", `${dir}/${v}.json`, `${dir}/${v}`, body, "--tele", `${dir}/${v}.tele`]);
    if (r.exitCode) throw new Error(`rendering the lint duet: ${r.stderr.toString()}`);
  }
  // a long title, to see it trimmed on a phone
  const voices = [{ name: "Lower", tele: "low.tele", melody: "low.json" }, { name: "Upper", tele: "high.tele", melody: "high.json" }];
  writeFileSync(`${dir}/show.json`, JSON.stringify({ title: "A duet with a title long enough to need trimming", audio: "low.wav", bpm: 120, voices }));
}

async function main() {
  const out = arg("out") ?? "out/layout";
  rmSync(out, { recursive: true, force: true });
  mkdirSync(out, { recursive: true });
  let url = arg("url");
  let server: { close(): Promise<void> } | undefined;
  if (!url) {
    const { createServer } = await import("vite");
    const s = await createServer({ server: { port: 0, strictPort: false, host: "127.0.0.1" }, logLevel: "error" });
    await s.listen();
    server = s;
    url = s.resolvedUrls!.local[0];
  }
  const only = arg("only");
  const onlyMode = arg("mode");
  const viewports = only ? VIEWPORTS.filter((v) => `${v.w}x${v.h}` === only).concat(VIEWPORTS.some((v) => `${v.w}x${v.h}` === only) ? [] : [{ w: +only.split("x")[0], h: +only.split("x")[1] }]) : VIEWPORTS;
  const duet = !onlyMode || onlyMode === "duet";
  const modes = onlyMode ? (onlyMode === "duet" ? [] : [onlyMode]) : MODES;
  const duetDir = `${out}/duet-show`;
  if (duet) makeDuet(duetDir);

  // WebGL for the 3D face: the GPU through Metal on a Mac; elsewhere SwiftShader, which draws on the
  // CPU (it took 6-15x the CPU time on an M4 for the same report, enough to stall the machine)
  const gl = process.platform === "darwin" ? ["--use-angle=metal", "--ignore-gpu-blocklist"] : ["--use-angle=swiftshader", "--enable-unsafe-swiftshader"];
  const browser = await chromium.launch({ headless: true, args: ["--autoplay-policy=no-user-gesture-required", ...gl] });
  let errors = 0, warnings = 0, views = 0;
  for (const { w, h } of viewports) {
    const page = await browser.newPage({ viewport: { width: w, height: h }, deviceScaleFactor: 1 });
    await page.addInitScript(hookCanvasText);
    page.on("pageerror", (e) => {
      console.log(`  pageerror at ${w}x${h}: ${e.message}`);
      errors++;
    });
    const check = async (state: string, wait = 350) => {
      await page.evaluate(() => document.fonts.ready);
      await page.waitForTimeout(wait);
      views++;
      const problems = (await page.evaluate(inspect)) as Problem[];
      const errs = problems.filter((p) => !p.warn), warns = problems.filter((p) => p.warn);
      errors += errs.length;
      warnings += warns.length;
      if (!problems.length) return;
      console.log(`\n${w}x${h} ${state}: ${errs.length} error(s), ${warns.length} warning(s)`);
      for (const p of [...errs, ...warns]) console.log(`  ${p.warn ? "warn " : "ERROR"} ${p.kind.padEnd(11)} ${p.detail}`);
      if (errs.length) await page.screenshot({ path: `${out}/${w}x${h}-${state.replace(/[^a-z0-9]+/gi, "-")}.png`, fullPage: true });
    };
    const mode = (m: string) => page.getByRole("radio", { name: new RegExp(`^${m}$`, "i") }).click();
    // every encoder page of the screens on the right (each page has its own labels)
    const pages = async (state: string) => {
      const tabs = page.locator(".right .enc-tab");
      for (let i = 1; i < (await tabs.count()); i++) {
        await tabs.nth(i).click();
        await check(`${state}, controls page ${(await tabs.nth(i).textContent())?.trim()}`);
      }
      if (await tabs.count()) await tabs.first().click();
    };
    if (duet) {
      // the duet page: two 3D heads, loaded and then playing
      await page.route("**/shows/lint-duet/*", (route) => route.fulfill({ path: `${duetDir}/${basename(new URL(route.request().url()).pathname)}` }));
      await page.goto(new URL("?show=lint-duet", url).href, { waitUntil: "networkidle" });
      await page.waitForSelector(".duet-head canvas", { timeout: 15000 });
      await check("duet", 2000);
      await page.locator(".duet-bar button.primary").click();
      // 1.2 s in: both heads singing, a word at each mouth
      await check("duet, playing", 1200);
    }
    if (!modes.length) {
      await page.close();
      continue;
    }
    await page.goto(url, { waitUntil: "networkidle" });
    for (const m of modes) {
      await mode(m);
      await check(m);
    }
    await page.locator(".topbar button.primary").click();
    await page.waitForSelector(".load", { timeout: 15000 });
    for (const m of modes) {
      await mode(m);
      await check(`${m} (running)`);
      await pages(`${m} (running)`);
      if (m === "play") {
        // the 3D face lays its readouts out on its own
        await page.locator(".view-toggles .toggle", { hasText: "3D" }).click();
        await check("play (running), 3D", 2500);
        await page.locator(".view-toggles .toggle", { hasText: "3D" }).click();
      }
      if (m === "speak") {
        // the gestural score with a phrase in it, mid-utterance
        await page.getByRole("button", { name: /^Speak$/ }).click();
        await check("speak (running), speaking", 900);
      }
      if (m === "bestiary") {
        // each creature has its own names and parameters
        const specimens = page.locator(".s-gallery .specimen");
        for (let i = 1; i < (await specimens.count()); i++) {
          await specimens.nth(i).click();
          const name = (await specimens.nth(i).locator(".specimen-name").textContent())?.trim();
          await check(`bestiary (running), ${name}`);
          await pages(`bestiary (running), ${name}`);
        }
        await specimens.first().click();
      }
    }
    await page.close();
  }
  await browser.close();
  await server?.close();
  console.log(`\nlayout lint: ${errors} error(s), ${warnings} warning(s) in ${views} views (${viewports.length} sizes)`);
  process.exit(errors ? 1 : 0);
}

await main();
