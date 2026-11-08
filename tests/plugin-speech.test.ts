/**
 * In the plugin Rust says the lines, and the page follows. Found by review: a reply dropped as stale
 * left the previous line's words showing, although Rust had already replaced that line.
 */
import { expect, mock, test } from "bun:test";
import { readFileSync } from "node:fs";
import { gunzipSync } from "node:zlib";
import { initEngine } from "./harness.ts";

await initEngine();
// the real engine's module needs the bundler for its worklet; this only needs a clock
let clock = 0;
mock.module("../src/audio/AudioEngine.ts", () => ({ engine: { onPlanned: () => () => undefined, voiceClock: () => clock } }));
const { EnglishG2P, planPhrases } = await import("../src/wasm/engine.ts");
const { followSpoken, spokenWordNow, currentPlan } = await import("../src/ui/speech.ts");
const { wordSpans } = await import("../src/ui/words.ts");

const g2p = new EnglishG2P();
g2p.setDictionaryText(gunzipSync(readFileSync(new URL("../public/data/cmudict.txt.gz", import.meta.url))).toString());
const phrases = g2p.toPhrases("Hello there.");
const plan = planPhrases(phrases, { body: { female: 0, child: 0 }, t0: 0 });
const T0 = 10;
/** The voice clock in the middle of the first word. */
const first = wordSpans(phrases, plan)[0];
const DURING_FIRST_WORD = T0 + (first.t0 + first.t1) / 2;

/** What the plugin answers next. */
function replies(body: unknown, ok = true): void {
  globalThis.fetch = (async () => ({ ok, json: async () => body })) as unknown as typeof fetch;
}

test("the words shown are those of the line sounding, and go when it's replaced", async () => {
  clock = DURING_FIRST_WORD;
  replies({ generation: 7, phrases, plan });
  await followSpoken(1, 0, 7, T0);
  expect(currentPlan()?.offset).toBe(T0);
  expect(spokenWordNow()?.text).toBe("hello");

  // a new line started, then the text changed: its plan is from another generation
  replies({ generation: 8, phrases, plan });
  await followSpoken(2, 1, 7, T0);
  expect(currentPlan()?.id).toBe(1);
  expect(spokenWordNow()).toBeNull();

  // a good reply brings words back
  replies({ generation: 8, phrases, plan });
  await followSpoken(3, 0, 8, T0);
  expect(currentPlan()?.id).toBe(3);
  expect(spokenWordNow()?.text).toBe("hello");

  // no such plan any more
  replies(null, false);
  await followSpoken(4, 5, 8, T0);
  expect(spokenWordNow()).toBeNull();

  // and a stop
  replies({ generation: 8, phrases, plan });
  await followSpoken(5, 0, 8, T0);
  expect(spokenWordNow()?.text).toBe("hello");
  await followSpoken(6, -1, 0, 0);
  expect(spokenWordNow()).toBeNull();
});

test("a reply that arrives after the next line started is dropped", async () => {
  let answer: (v: unknown) => void = () => undefined;
  globalThis.fetch = (() => new Promise((res) => (answer = res))) as unknown as typeof fetch;
  const slow = followSpoken(10, 0, 8, T0);
  const reply = { ok: true, json: async () => ({ generation: 8, phrases, plan }) };
  const first = answer;
  replies({ generation: 8, phrases, plan });
  await followSpoken(11, 0, 8, T0 + 1);
  first(reply);
  await slow;
  expect(currentPlan()?.id).toBe(11);
  expect(currentPlan()?.offset).toBe(T0 + 1);
});
