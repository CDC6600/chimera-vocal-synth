#!/usr/bin/env python3
"""Drive fitted tract resonances from the original recording's spectral focus."""

import argparse
import json
import math
from pathlib import Path

import numpy as np
from scipy.io import wavfile
from scipy.ndimage import gaussian_filter1d, median_filter
from scipy.signal import resample_poly, stft


def focus_contour(frequencies, power, low=1600, high=3300, min_share=0.22, min_contrast=4):
    envelope = gaussian_filter1d(power, 4, axis=0)
    band = np.flatnonzero((frequencies >= low) & (frequencies <= high))
    indices = band[np.argmax(envelope[band], axis=0)]
    centers, strengths = [], []
    for i, peak in enumerate(indices):
        center = frequencies[peak]
        near = abs(frequencies - center) < 90
        surrounding = (abs(frequencies - center) >= 180) & (abs(frequencies - center) < 350)
        contrast = 10 * np.log10(max(envelope[peak, i], 1e-20)
                                / max(float(np.max(envelope[surrounding, i])), 1e-20))
        total = max(float(power[(frequencies > 800) & (frequencies < 3600), i].sum()), 1e-20)
        share = float(power[near, i].sum()) / total
        weighted = float(np.sum(frequencies[near] * power[near, i])) / max(float(power[near, i].sum()), 1e-20)
        centers.append(np.clip(weighted, low, high))
        strengths.append(float(share > min_share and contrast > min_contrast))
    strength = gaussian_filter1d(median_filter(np.array(strengths), size=5), 2)
    center = gaussian_filter1d(median_filter(np.array(centers), size=5), 1.2)
    return center, strength


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("track", type=Path)
    parser.add_argument("reference", type=Path)
    parser.add_argument("postures", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    track = json.loads(args.track.read_text())
    postures = json.loads(args.postures.read_text())
    if len(postures) < 2 or any(p["contrast"] < 3 for p in postures):
        parser.error("postures must contain at least two focused tract responses with contrast >= 3 dB")
    postures.sort(key=lambda p: p["target"])
    targets = np.array([p["target"] for p in postures])
    if np.any(np.diff(targets) <= 0):
        parser.error("posture frequencies must be distinct")
    fs, audio = wavfile.read(args.reference)
    if np.issubdtype(audio.dtype, np.integer):
        audio = audio.astype(float) / (np.iinfo(audio.dtype).max + 1)
    if audio.ndim == 2:
        audio = audio.mean(axis=1)
    duration = len(audio) / fs
    if abs(duration - len(track["frames"]) / track["rate"]) > 0.021:
        parser.error("reference and control track must cover the same interval")
    divisor = math.gcd(fs, 16000)
    audio = resample_poly(audio, 16000 // divisor, fs // divisor)
    frequencies, times, spectrum = stft(audio, 16000, nperseg=2048, noverlap=1728, boundary="zeros")
    power = abs(spectrum) ** 2
    centers, strengths = focus_contour(frequencies, power)
    low_centers, low_strengths = focus_contour(frequencies, power, 900, 1600, 0.45, 6)
    clock = np.arange(len(track["frames"])) / track["rate"]
    base_gate = np.array([f["gate"] for f in track["frames"]])
    uncertain = median_filter((np.interp(times, clock, base_gate) < 0.35).astype(float), size=21)
    low_strengths *= uncertain * (1 - strengths)
    combined = strengths + low_strengths
    centers = np.where(combined > 0.01,
                       (centers * strengths + low_centers * low_strengths) / np.maximum(combined, 0.01), centers)
    strengths = combined
    low_strengths = np.interp(clock, times, low_strengths)
    centers = np.interp(clock, times, centers)
    strengths = np.interp(clock, times, strengths)
    level = np.sqrt(np.sum(power, axis=0))
    envelope = np.interp(clock, times, np.clip(level / max(float(np.percentile(level, 90)), 1e-9), 0, 1))
    keys = list(postures[0]["artic"])
    rows = []
    last_pitch = track["frames"][0]["f0"]
    for i, frame in enumerate(track["frames"]):
        if frame["gate"] > 0.55:
            last_pitch += 0.08 * (frame["f0"] - last_pitch)
        weight = float(strengths[i])
        if weight > 0.02 and frame["gate"] < 0.35:
            # Separation can discard a focused overtone. Keep the last reliable low pitch.
            frame["f0"] = (1 - weight) * frame["f0"] + weight * last_pitch
        period_rate = frame["f0"] / 2
        frame["alternatingAttenuation"] = round(0.85 * (1 - float(low_strengths[i])), 4)
        if low_strengths[i] > 0.5:
            period_rate = frame["f0"]
        target = np.clip(round(centers[i] / period_rate) * period_rate, targets[0], targets[-1])
        articulation = frame["artic"]
        for key in keys:
            fitted = float(np.interp(target, targets, [p["artic"][key] for p in postures]))
            baseline = articulation.get(key, fitted)
            articulation[key] = round(baseline + weight * (fitted - baseline), 5)
        if weight > 0.02:
            frame["tipFree"] = False
        frame["gate"] = round(max(frame["gate"], weight * (0.5 + 0.5 * envelope[i])), 5)
        frame["f0"] = round(frame["f0"], 4)
        rows.append([round(float(clock[i]), 4), round(float(target), 2), round(weight, 4)])
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(track, separators=(",", ":")) + "\n")
    args.output.with_suffix(".focus.json").write_text(json.dumps({"columns": ["time", "resonanceHz", "weight"], "frames": rows}) + "\n")
    print(f"{duration:.2f} seconds; focused tract active for {np.mean(strengths > 0.5) * duration:.2f} seconds")
    print("Targets come from the original mix. Instrument harmonics can affect their estimates.")


if __name__ == "__main__":
    main()
