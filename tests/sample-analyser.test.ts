import { expect, test } from "bun:test";
import { SampleAnalyser } from "../src/audio/sampleAnalyser.ts";

const SR = 48000;
const sine = (f: number, n: number, amp = 1, from = 0) => Float32Array.from({ length: n }, (_, i) => amp * Math.sin((2 * Math.PI * f * (i + from)) / SR));

test("a sine shows at its bin, at the level the Web Audio spec gives", () => {
  const a = new SampleAnalyser(4096, 0);
  const f = 1000;
  a.push(sine(f, 4096));
  const out = new Float32Array(a.frequencyBinCount);
  a.getFloatFrequencyData(out);
  const peak = out.indexOf(Math.max(...out));
  expect(peak).toBe(Math.round((f / SR) * 4096));
  // a unit sine is amp/2 per side, and the Blackman window's mean is 0.42: 20 log10(0.21) = -13.6 dB
  expect(out[peak]).toBeGreaterThan(-15);
  expect(out[peak]).toBeLessThan(-13);
  // and little leaks far from it
  expect(out[peak + 100]).toBeLessThan(-100);
});

test("silence reads as nothing, not as a number that would draw a line", () => {
  const a = new SampleAnalyser(2048, 0);
  a.push(new Float32Array(2048));
  const out = new Float32Array(a.frequencyBinCount);
  a.getFloatFrequencyData(out);
  expect(out.every((x) => x === -Infinity)).toBe(true);
});

test("smoothing holds a sound that stopped; without it, it's gone", () => {
  const out = new Float32Array(1024);
  for (const [smoothing, stays] of [[0.6, true], [0, false]] as const) {
    const a = new SampleAnalyser(2048, smoothing);
    a.push(sine(1000, 2048));
    a.getFloatFrequencyData(out);
    const bin = Math.round((1000 / SR) * 2048);
    const loud = out[bin];
    a.push(new Float32Array(2048));
    a.getFloatFrequencyData(out);
    expect(out[bin] > loud - 20).toBe(stays);
  }
});

test("the newest samples win, however they arrive", () => {
  const a = new SampleAnalyser(1024, 0);
  a.push(sine(500, 1024));
  // in pieces that wrap the ring, then more than it holds at once
  const b = sine(5000, 4096);
  a.push(b.subarray(0, 300));
  a.push(b.subarray(300, 900));
  a.push(b.subarray(900, 4096));
  const out = new Float32Array(a.frequencyBinCount);
  a.getFloatFrequencyData(out);
  expect(out.indexOf(Math.max(...out))).toBe(Math.round((5000 / SR) * 1024));
});
