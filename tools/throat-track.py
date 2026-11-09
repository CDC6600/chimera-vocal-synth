#!/usr/bin/env python3
"""Estimate CHIMERA controls from an isolated throat-singing WAV, without copying samples."""

import argparse
import json
import math
from pathlib import Path

import numpy as np
from scipy.io import wavfile
from scipy.linalg import solve_toeplitz
from scipy.ndimage import gaussian_filter1d, median_filter
from scipy.signal import correlate, find_peaks, resample_poly


def pitch_candidates(frame, fs, fmin, fmax):
    half = len(frame) // 2
    reference = frame[:half]
    cross = correlate(frame, reference, mode="valid", method="fft")
    energy = np.r_[0.0, np.cumsum(frame * frame)]
    difference = np.maximum(0, np.sum(reference * reference)
                            + energy[half:] - energy[:-half] - 2 * cross)
    normalized = np.ones_like(difference)
    normalized[1:] = difference[1:] * np.arange(1, len(difference)) / np.maximum(
        np.cumsum(difference[1:]), 1e-12)
    lo, hi = int(fs / fmax), min(int(fs / fmin), len(normalized) - 2)
    dips, _ = find_peaks(-normalized[lo:hi + 1])
    dips += lo
    if not len(dips):
        dips = [lo + np.argmin(normalized[lo:hi + 1])]
    candidates = []
    for k in dips:
        a, b, c = normalized[k - 1:k + 2]
        denominator = a - 2 * b + c
        correction = np.clip(0.5 * (a - c) / denominator, -0.5, 0.5) if denominator else 0
        candidates.append((np.clip(fs / (k + correction), fmin, fmax), normalized[k]))
    return np.array(candidates)


def pitch(frame, fs, fmin, fmax):
    candidates = pitch_candidates(frame, fs, fmin, fmax)
    good = np.flatnonzero(candidates[:, 1] < 0.2)
    k = good[0] if len(good) else np.argmin(candidates[:, 1])
    return candidates[k, 0], 1 - candidates[k, 1]


def track_pitch(candidates, rms):
    """Choose a pitch path across frames instead of switching harmonics frame by frame."""
    level = max(float(np.percentile(rms, 90)), 1e-9)
    costs, parents = [], []
    for i, choices in enumerate(candidates):
        frequency, difference = choices.T
        reliability = np.clip(rms[i] / (0.15 * level), 0, 1)
        observation = reliability * (4 * difference + 0.06 * np.log2(140 / frequency))
        if i == 0:
            costs.append(observation)
            parents.append(np.zeros(len(choices), dtype=int))
            continue
        semitones = abs(12 * np.log2(frequency[:, None] / candidates[i - 1][:, 0]))
        transition = 0.10 * semitones + 0.06 * semitones ** 2
        total = costs[-1][None, :] + transition
        parent = np.argmin(total, axis=1)
        costs.append(observation + total[np.arange(len(choices)), parent])
        parents.append(parent)
    k = int(np.argmin(costs[-1]))
    selected = []
    for i in reversed(range(len(candidates))):
        selected.append(candidates[i][k])
        k = parents[i][k]
    selected = np.array(selected[::-1])
    reliable = (selected[:, 1] < 0.65) & (rms > level * 0.09)
    if not reliable.any():
        raise ValueError("no voiced frames found in this pitch range")
    times = np.arange(len(rms))
    log_pitch = np.interp(times, times[reliable], np.log2(selected[reliable, 0]))
    return 2 ** gaussian_filter1d(median_filter(log_pitch, size=3), 1.2), reliable


def breath_envelope(rms):
    """Low pitch confidence must not cut off an audible sustained phrase."""
    level = max(float(np.percentile(rms, 90)), 1e-9)
    relative = gaussian_filter1d(np.asarray(rms) / level, 0.8)
    audible = np.clip((relative - 0.035) / 0.085, 0, 1)
    audible = audible * audible * (3 - 2 * audible)
    return audible * (0.45 + 0.55 * np.clip(relative, 0, 1) ** 0.5)


def mouth_positions(table, resonances):
    vowels = [v for v in ["ɑ", "ɔ", "o", "u", "ʌ", "ə", "ɛ", "e", "i", "ɯ"] if v in table]
    targets = np.array([table[v]["formants"][:2] for v in vowels])
    keys = list(table[vowels[0]]["artic"])
    postures = np.array([[table[v]["artic"].get(key, 0) for key in keys] for v in vowels])
    last = np.array(table["ə"]["formants"][:2], dtype=float)
    estimates = []
    for resonance in resonances:
        if resonance is not None:
            last = resonance
        estimates.append(last)
    estimates = np.exp(gaussian_filter1d(np.log(estimates), 1.5, axis=0))
    distance = np.sum((np.log(estimates[:, None, :] / targets) * [1.0, 1.2]) ** 2, axis=2)
    weights = np.exp(-(distance - distance.min(axis=1, keepdims=True)) / 0.08)
    weights /= weights.sum(axis=1, keepdims=True)
    return keys, gaussian_filter1d(weights @ postures, 1.5, axis=0)


def formants(frame, fs):
    emphasized = (frame[1:] - 0.97 * frame[:-1]) * np.hanning(len(frame) - 1)
    autocorrelation = correlate(emphasized, emphasized, mode="full", method="fft")
    r = autocorrelation[len(emphasized) - 1:len(emphasized) + 18]
    r[0] += max(1e-10, r[0] * 1e-6)
    try:
        coefficients = solve_toeplitz(r[:18], -r[1:19])
        roots = np.roots(np.r_[1, coefficients])
    except np.linalg.LinAlgError:
        return None
    roots = roots[np.imag(roots) > 0]
    frequencies = np.angle(roots) * fs / (2 * np.pi)
    bandwidths = -fs / np.pi * np.log(np.maximum(np.abs(roots), 1e-12))
    valid = np.sort(frequencies[(frequencies > 230) & (frequencies < 3500)
                                & (bandwidths > 0) & (bandwidths < 600)])
    if len(valid) < 2:
        return None
    first = valid[valid < 1050]
    if not len(first):
        return None
    second = valid[(valid > max(650, first[0] + 250)) & (valid < 2600)]
    return np.array([first[0], second[0]]) if len(second) else None


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("vocals", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--fmin", type=float, default=45)
    parser.add_argument("--fmax", type=float, default=140)
    parser.add_argument("--fold-ratio", type=int, choices=[1, 2], default=2,
                        help="2 drives alternating fold cycles at twice the measured period rate")
    args = parser.parse_args()
    if not 20 <= args.fmin < args.fmax <= 1000:
        parser.error("pitch limits must satisfy 20 <= fmin < fmax <= 1000")
    fs, audio = wavfile.read(args.vocals)
    if np.issubdtype(audio.dtype, np.integer):
        audio = audio.astype(float) / (np.iinfo(audio.dtype).max + 1)
    else:
        audio = audio.astype(float)
    if audio.ndim == 2:
        audio = audio.mean(axis=1)
    if not len(audio) or not np.isfinite(audio).all():
        parser.error("audio must contain finite samples")
    divisor = math.gcd(fs, 8000)
    audio = resample_poly(audio, 8000 // divisor, fs // divisor)
    fs, rate, hop = 8000, 50, 160
    count = math.ceil(len(audio) / hop)
    width = max(800, int(4 * fs / args.fmin))
    width += width % 2
    padded = np.pad(audio, (width // 2, width // 2 + hop))
    candidates, rms, resonances = [], [], []
    for i in range(count):
        frame = padded[i * hop:i * hop + width]
        candidates.append(pitch_candidates(frame - frame.mean(), fs, args.fmin, args.fmax))
        rms.append(np.sqrt(np.mean(frame * frame)))
        resonances.append(formants(frame, fs))

    rms = np.array(rms)
    try:
        f0, active = track_pitch(candidates, rms)
    except ValueError as error:
        parser.error(str(error))
    gate = breath_envelope(rms)

    table_path = Path(__file__).resolve().parents[1] / "crates/chimera/src/phonetics/vowelTargets.generated.json"
    table = json.loads(table_path.read_text())["male"]
    keys, positions = mouth_positions(table, resonances)
    # Interpolate the estimates to 5 ms targets; the engine also smooths pitch and articulation.
    output_times = np.arange(count * 4) / 4
    f0 = 2 ** np.interp(output_times, np.arange(count), np.log2(f0))
    gate = np.interp(output_times, np.arange(count), gate)
    positions = np.array([np.interp(output_times, np.arange(count), column)
                          for column in positions.T]).T
    count *= 4
    rate *= 4
    frames = []
    for i in range(count):
        artic = {key: round(float(value), 4) for key, value in zip(keys, positions[i])}
        artic.update(epilarynx=0.65, ventricular=0.6, velum=0)
        frames.append({"f0": round(float(f0[i] * args.fold_ratio), 3),
                       "gate": round(float(gate[i]), 4), "artic": artic})
    track = {
        "rate": rate,
        "source": {"glide": 0.02, "pressure": 9.5, "breathiness": 0.025,
                   "tension": 0.85, "roughness": 0.1, "brightness": 0.5,
                   "fry": 0, "vibratoDepth": 0, "drift": 0},
        "phonation": {"diplophonia": 0.0,
                      "alternatingAttenuation": 0.85 if args.fold_ratio == 2 else 0.0,
                      "jitter": 0.003, "shimmer": 0.02},
        "frames": frames,
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(track, ensure_ascii=False, separators=(",", ":")) + "\n")
    print(f"{count / rate:.2f} s, {count} frames, {active.mean():.1%} reliable pitch estimates")
    print(f"Measured period rate: {np.percentile(f0[::4][active], [5, 50, 95]).round(1)} Hz (5th, 50th, 95th percentiles)")
    print("Vowels approximate the spectral envelope. This does not transcribe lyrics.")


if __name__ == "__main__":
    main()
