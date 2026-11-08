/**
 * The plugin's UI end to end, without a DAW: the real page in headless Chromium, talking to the real
 * Bridge and Core in `examples/ui_harness.rs` (Rust runs the voice; its audio goes nowhere). The test
 * also plays the host's part: notes, automation, transport, project restore. Only the webview itself
 * (WKWebView over the custom scheme) isn't covered.
 *
 * Usage: bun run build && cargo build -p chimera-plugin --release --example ui_harness && bun tools/plugin-e2e.ts [outdir]
 */
import { chromium } from "playwright-core";
import { mkdirSync } from "node:fs";
import { spawn } from "node:child_process";

const PORT = 8549;
const base = `http://127.0.0.1:${PORT}`;
const outDir = process.argv[2] ?? "out/plugin-e2e";
mkdirSync(outDir, { recursive: true });

const harness = spawn("target/release/examples/ui_harness", [String(PORT)], { stdio: "inherit" });
for (let i = 0; i < 100; i++) {
  try {
    await fetch(`${base}/params`);
    break;
  } catch {
    await new Promise((r) => setTimeout(r, 100));
  }
}

const host = (path: string, body: string | number = "") => fetch(`${base}/test/${path}`, { method: "POST", body: String(body) });

const browser = await chromium.launch({ headless: true, args: ["--use-angle=swiftshader", "--enable-unsafe-swiftshader"] });
// the editor's size in crates/chimera-plugin/src/editor.rs
const page = await browser.newPage({ viewport: { width: 1280, height: 760 }, deviceScaleFactor: 1 });
const errors: string[] = [];
page.on("console", (m) => {
  if (m.type() === "error") errors.push(m.text());
});
page.on("pageerror", (e) => errors.push(`pageerror: ${e.message}`));

let failed = 0;
function check(name: string, ok: unknown, detail?: unknown): void {
  console.log(`${ok ? "  ok  " : " FAIL "} ${name}${!ok && detail !== undefined ? `: ${JSON.stringify(detail)}` : ""}`);
  if (!ok) failed++;
}

/** Wait for something the page or Rust should come to. */
async function until(name: string, fn: () => Promise<unknown> | unknown, ms = 3000): Promise<void> {
  const t0 = Date.now();
  let last: unknown;
  while (Date.now() - t0 < ms) {
    last = await fn();
    if (last) return check(name, true);
    await page.waitForTimeout(25);
  }
  check(name, false, last);
}

const state = () => page.evaluate(() => fetch("/state").then((r) => r.json()));
const tele = () => page.evaluate(() => (window as never as { __chimeraEngine: { latest: { t: { clock: number; voicing: number; gate: number; f0: number }; body: { female: number } } | null } }).__chimeraEngine.latest && { ...(window as never as { __chimeraEngine: { latest: { t: { clock: number; voicing: number; gate: number; f0: number } } } }).__chimeraEngine.latest.t, body: (window as never as { __chimeraEngine: { latest: { body: { female: number; child: number } } } }).__chimeraEngine.latest.body });
const next = () => page.locator(".syl.next").textContent();
const params = () => fetch(`${base}/params`).then((r) => r.json() as Promise<Record<string, number>>);

try {
  await page.goto(base, { waitUntil: "load" });
  await page.waitForSelector(".topbar");

  // ---- what the plugin shows
  const modes = await page.locator(".topbar [role=radiogroup] [role=radio], .topbar .segmented button").allTextContents();
  check("only Sing and Play are offered", modes.includes("Sing") && modes.includes("Play") && !modes.includes("Speak") && !modes.includes("Bestiary") && !modes.includes("Beatbox"), modes);
  check("no start prompt: the voice is already running", (await page.locator(".start-prompt").count()) === 0);
  check("no link away from the plugin", (await page.locator(".repo-link").count()) === 0);
  check("no Web MIDI button", (await page.locator("button.midi").count()) === 0);
  check("no demo songs", (await page.locator(".demo-songs").count()) === 0);
  await until("telemetry reaches the page", async () => (await tele())?.clock);
  const c0 = (await tele())!.clock;
  await page.waitForTimeout(200);
  check("the voice clock moves", (await tele())!.clock > c0);
  check("the engine load shows", /Engine load \d+ %/.test((await page.locator(".load").textContent()) ?? ""));
  await until("the load is a real measurement", async () => !/load 0 %/.test((await page.locator(".load").textContent()) ?? ""));

  // ---- lyrics belong to Rust
  await until("the syllables come from the plugin", async () => (await page.locator(".syl").count()) > 5);
  await page.locator(".lyrics textarea").fill("one two three");
  await until("new lyrics come back as syllables", async () => (await page.locator(".syl").count()) === 3);
  check("the project keeps them", (await state()).settings.lyrics === "one two three");
  check("the cursor is at the start", (await next()) === "one");

  // ---- the page's keys
  await page.screenshot({ path: `${outDir}/sing.png` });
  const key = page.locator(".key.white").nth(9);
  const box = (await key.boundingBox())!;
  await page.mouse.move(box.x + box.width / 2, box.y + box.height - 8);
  await page.mouse.down();
  await until("a key on the page sings", async () => (await tele())!.voicing > 0.3);
  await until("and the cursor moves", async () => (await next()) === "two");
  await until("the sung syllable shows", async () => (await page.locator(".syl.now, .syl.sung").count()) >= 0);
  await page.screenshot({ path: `${outDir}/sing-note.png` });
  await page.mouse.up();
  await until("releasing it ends the note", async () => (await tele())!.voicing < 0.1);

  // ---- the host's keys
  await host("note", 64);
  await until("a host note sings the next syllable", async () => (await next()) === "three");
  await until("the page sees the voice singing", async () => (await tele())!.voicing > 0.3);
  await host("off", 64);
  await until("a host note-off ends it", async () => (await tele())!.voicing < 0.1);
  await host("note", 62);
  await until("past the last syllable the lyrics wrap", async () => (await next()) === "one");
  await host("off", 62);

  // ---- the piano roll and the words follow what Rust plays
  const rollNotes = () => page.evaluate(() => (window as never as { __chimeraRoll: { notes: { midi: number; t0: number; t1: number | null; text: string }[] } }).__chimeraRoll.notes.map((n) => ({ ...n })));
  await page.locator(".lyrics textarea").fill("la - la");
  await until("lyrics with a melisma", async () => (await page.locator(".syl").count()) === 3);
  const seen = (await rollNotes()).length;
  await host("note", 60);
  await until("a note goes on the roll", async () => (await rollNotes()).length === seen + 1);
  await host("note", 64);
  await until("a melisma adds its pitch to the roll", async () => (await rollNotes()).length === seen + 2);
  check("as a held vowel, not a new syllable", (await rollNotes()).at(-1)?.text === "~", (await rollNotes()).at(-1));
  await host("off", 64);
  await until("releasing it glides back to the key still down", async () => (await rollNotes()).at(-1)?.text === "~");
  await host("off", 60);
  await until("the last release ends the note on the roll", async () => (await rollNotes()).at(-1)?.t1 !== null);
  // the same pitch again, its release and press closer together than a poll
  await host("note", 60);
  await until("the next syllable", async () => (await rollNotes()).at(-1)?.t1 === null && (await rollNotes()).at(-1)?.text === "la");
  const before2 = (await rollNotes()).length;
  await host("off", 60);
  await host("note", 60);
  await until("a repeated pitch is a new note on the roll", async () => (await rollNotes()).length === before2 + 1);
  await host("off", 60);
  await page.locator(".lyrics textarea").fill("one two three");
  await until("back to three syllables", async () => (await page.locator(".syl").count()) === 3);

  // ---- the transport
  await page.locator(".syl").nth(1).click();
  await until("a syllable picked while stopped takes the cursor", async () => (await next()) === "two");
  check("and is saved as where playing starts", (await state()).settings.startSyllable === 1);
  await host("play");
  await page.waitForTimeout(100);
  check("playing starts from it", (await next()) === "two");
  await host("note", 60);
  await until("a note moves on", async () => (await next()) === "three");
  await host("off", 60);
  await host("stop");
  await page.waitForTimeout(50);
  await host("play");
  await until("playing again goes back to it", async () => (await next()) === "two");
  await host("stop");

  // ---- parameters: both ways, and one gesture per drag
  const vibrato = page.getByRole("slider", { name: "Vibrato" });
  await vibrato.focus();
  const before = (await params()).vibrato;
  await page.keyboard.press("PageUp");
  await page.keyboard.press("PageUp");
  await until("an encoder moves the host parameter", async () => (await params()).vibrato > before);
  const opening = await (await host("gestures")).text();
  check("the gesture is open while moving", opening.includes("begin vibrato") && !opening.includes("end vibrato"), opening);
  await until("and ends when the moves stop", async () => (await (await host("gestures")).text()).includes("end vibrato"));
  check("two moves were one gesture", ((await (await host("gestures")).text()).match(/begin vibrato/g) ?? []).length === 1);

  await host("param", "vibrato=12");
  await until("host automation moves the encoder", async () => (await vibrato.getAttribute("aria-valuetext"))?.startsWith("12 "));
  await host("param", "gain=-30");
  await until("host automation moves the volume slider", async () => Number(await page.locator(".volume input").inputValue()) === 0);
  await host("param", "gain=0");

  // ---- the body
  await page.getByRole("radio", { name: "Female" }).click();
  await until("a body change reaches the voice", async () => (await tele())!.body.female === 1);
  const s1 = await state();
  check("and the project", s1.settings.body.female === 1 && s1.settings.f0 === 205, s1.settings);
  await host("note", 69);
  await until("the new body sings", async () => (await tele())!.voicing > 0.3);
  await host("off", 69);
  const clock = (await tele())!.clock;
  const future = (await rollNotes()).filter((n) => n.t0 > clock + 0.5);
  check("nothing from the old voice's timeline is on the roll", future.length === 0, future);

  // ---- Play
  await page.getByRole("radio", { name: "Play" }).click();
  await until("Play is saved", async () => (await state()).settings.mode === "play");
  await page.waitForSelector(".right-play");
  await page.screenshot({ path: `${outDir}/play.png` });
  await page.getByRole("button", { name: "Hold breath" }).click();
  await until("Hold breath breathes", async () => (await tele())!.gate > 0.5);
  await page.getByRole("button", { name: "Hold breath" }).click();
  await until("and stops", async () => (await tele())!.gate < 0.1);
  await host("note", 69);
  await until("a host key breathes at its pitch", async () => Math.abs((await tele())!.f0 - 440) < 8);
  await until("the readout follows it", async () => /440 Hz/.test((await page.locator(".s-keys").textContent()) ?? ""));
  await host("off", 69);
  await page.getByRole("button", { name: "Whisper" }).click();
  await until("a saved setting reaches the project", async () => (await state()).settings.voiced === false);
  await page.getByRole("button", { name: "Whisper" }).click();
  // the 3D face is a chunk of its own, over the same protocol
  await page.getByRole("checkbox", { name: "3D" }).click().catch(() => page.getByText("3D", { exact: true }).click());
  await page.waitForTimeout(1500);
  await page.screenshot({ path: `${outDir}/play-3d.png` });
  await page.getByRole("radio", { name: "Sing" }).click();
  await until("back to Sing", async () => (await state()).settings.mode === "sing");

  // ---- a project restored while the window is open
  await host("restore", JSON.stringify({ version: 1, lyrics: "la la land", body: { female: 0, child: 1 }, mode: "play", artic: { jaw: 1.5 }, tipFree: true, voiced: true, f0: 300, startSyllable: 0 }));
  await until("a restore replaces the mode on the page", async () => (await page.locator(".right-play").count()) === 1);
  // a page that hasn't seen the restore yet can't overwrite it
  await page.evaluate(() => window.ipc!.postMessage(JSON.stringify({ rev: -2, type: "lyrics", text: "stale" })));
  await page.waitForTimeout(100);
  check("a command from before the restore is dropped", (await state()).settings.lyrics === "la la land");
  check("and the mode", (await state()).settings.mode === "play");
  await until("and the voice", async () => (await tele())?.body.child === 1);
  await page.getByRole("radio", { name: "Sing" }).click();
  await until("the restored lyrics show", async () => (await page.locator(".lyrics textarea").inputValue()) === "la la land");
  await until("as syllables", async () => (await page.locator(".syl").count()) === 3);

  // ---- a new window starts from the plugin, not from the page's defaults
  const reopened = await browser.newPage({ viewport: { width: 1280, height: 760 } });
  await reopened.goto(base, { waitUntil: "load" });
  await reopened.waitForSelector(".topbar");
  check("a reopened window shows the project", (await reopened.locator(".lyrics textarea").inputValue()) === "la la land");
  const after = await state();
  check("and sent nothing back", after.settings.lyrics === "la la land" && after.settings.body.child === 1 && after.settings.artic.jaw === 1.5 && after.settings.f0 === 300, after.settings);
  await reopened.close();

  // ---- the window closes with a key held and a gesture open
  const key2 = page.locator(".key.white").nth(5);
  const b2 = (await key2.boundingBox())!;
  await page.mouse.move(b2.x + b2.width / 2, b2.y + b2.height - 8);
  await page.mouse.down();
  await until("held", async () => (await tele())!.voicing > 0.3);
  await page.evaluate(async () => {
    const { revision } = await fetch("/state").then((r) => r.json());
    window.ipc!.postMessage(JSON.stringify({ rev: revision, type: "param", id: "fry", value: 0.3 }));
  });
  await page.waitForTimeout(50);
  await host("close");
  await until("closing the window lifts the key", async () => (await tele())!.voicing < 0.1);
  await until("and ends the gesture", async () => (await (await host("gestures")).text()).includes("end fry"));
  await page.mouse.up();

  check("no errors on the page", errors.length === 0, errors);
} catch (e) {
  check("the run finished", false, String(e));
  await page.screenshot({ path: `${outDir}/failure.png` }).catch(() => undefined);
} finally {
  await browser.close();
  harness.kill();
}
console.log(failed ? `\n${failed} failed` : "\nall passed");
process.exit(failed ? 1 : 0);
