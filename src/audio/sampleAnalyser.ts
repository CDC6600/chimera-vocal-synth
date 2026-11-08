/**
 * The part of `AnalyserNode` the spectrum views use, for the plugin, which has no Web Audio: the
 * audio thread's samples go in, and `getFloatFrequencyData` follows the Web Audio spec (Blackman
 * window, FFT, smoothing over time, decibels), so the views don't know the difference.
 */

export interface Analyser {
  readonly fftSize: number;
  readonly frequencyBinCount: number;
  getFloatFrequencyData(out: Float32Array): void;
}

export class SampleAnalyser implements Analyser {
  readonly frequencyBinCount: number;
  private ring: Float32Array;
  private at = 0;
  private window: Float64Array;
  private re: Float64Array;
  private im: Float64Array;
  private smoothed: Float64Array;

  constructor(
    readonly fftSize: number,
    private readonly smoothing: number,
  ) {
    this.frequencyBinCount = fftSize / 2;
    this.ring = new Float32Array(fftSize);
    this.window = new Float64Array(fftSize);
    // Blackman, alpha 0.16, as the spec has it
    const a0 = 0.42, a1 = 0.5, a2 = 0.08;
    for (let i = 0; i < fftSize; i++) {
      const x = (i / fftSize) * 2 * Math.PI;
      this.window[i] = a0 - a1 * Math.cos(x) + a2 * Math.cos(2 * x);
    }
    this.re = new Float64Array(fftSize);
    this.im = new Float64Array(fftSize);
    this.smoothed = new Float64Array(this.frequencyBinCount);
  }

  /** The newest samples. */
  push(samples: Float32Array): void {
    const n = this.fftSize;
    const from = Math.max(0, samples.length - n);
    for (let i = from; i < samples.length; i++) {
      this.ring[this.at] = samples[i];
      this.at = (this.at + 1) % n;
    }
  }

  getFloatFrequencyData(out: Float32Array): void {
    const n = this.fftSize;
    const { re, im, window, ring, smoothed, smoothing } = this;
    for (let i = 0; i < n; i++) {
      re[i] = ring[(this.at + i) % n] * window[i];
      im[i] = 0;
    }
    fft(re, im);
    const count = Math.min(out.length, this.frequencyBinCount);
    for (let k = 0; k < count; k++) {
      const mag = Math.hypot(re[k], im[k]) / n;
      smoothed[k] = smoothing * smoothed[k] + (1 - smoothing) * mag;
      out[k] = smoothed[k] > 0 ? 20 * Math.log10(smoothed[k]) : -Infinity;
    }
  }
}

/** In-place radix-2 FFT; the length is a power of two. */
function fft(re: Float64Array, im: Float64Array): void {
  const n = re.length;
  for (let i = 1, j = 0; i < n; i++) {
    let bit = n >> 1;
    for (; j & bit; bit >>= 1) j ^= bit;
    j ^= bit;
    if (i < j) {
      [re[i], re[j]] = [re[j], re[i]];
      [im[i], im[j]] = [im[j], im[i]];
    }
  }
  for (let len = 2; len <= n; len *= 2) {
    const angle = (-2 * Math.PI) / len;
    const stepRe = Math.cos(angle), stepIm = Math.sin(angle);
    for (let i = 0; i < n; i += len) {
      let wRe = 1, wIm = 0;
      for (let k = 0; k < len / 2; k++) {
        const a = i + k, b = a + len / 2;
        const tRe = re[b] * wRe - im[b] * wIm, tIm = re[b] * wIm + im[b] * wRe;
        re[b] = re[a] - tRe;
        im[b] = im[a] - tIm;
        re[a] += tRe;
        im[a] += tIm;
        const next = wRe * stepRe - wIm * stepIm;
        wIm = wRe * stepIm + wIm * stepRe;
        wRe = next;
      }
    }
  }
}
