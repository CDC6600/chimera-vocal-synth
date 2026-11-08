"""Speech intelligibility of the engine, scored by Whisper: render each sentence with
`chimera render text` in the male and female voices, transcribe it with faster-whisper
(small.en, greedy, so the same audio gives the same words) and count word errors.

usage: speech_wer.py <outdir> [--against earlier/wer.json]

Writes <outdir>/wer.json and the renders. With --against it compares the training sentences'
word error rate: BETTER if it fell by at least MARGIN points, WORSE if it rose by more than
SLACK, and NO CLEAR CHANGE between. The held-out sentences are reported but never decide.

Set up once with tools/asr/setup.sh (a Python env and the model in ~/.cache/chimera-asr).
The sentences are in sentences.tsv (kind, text, set): Harvard sentences (IEEE 1969) and
semantically unpredictable ones, which Whisper can't guess from context. "train" sentences are
the ones a tuning loop may listen to; "held" ones (Harvard list 2, and unpredictable sentences
dense in sh, ch and j) only check that gains carry over.
"""

import json
import os
import re
import subprocess
import sys
import wave
from concurrent.futures import ThreadPoolExecutor

import numpy as np
from faster_whisper import WhisperModel

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))
CACHE = os.path.expanduser("~/.cache/chimera-asr/models")
BODIES = ("male", "female")
# word error rate (points) a change may add before it counts as a speech regression
SLACK = 3.0
# fall in word error rate (points) that counts as an improvement: about 4 of the ~290 training words
MARGIN = 1.5


def words(text):
    text = re.sub(r"[^a-z' ]+", " ", text.lower().replace("’", "'"))
    return [w.strip("'") for w in text.split() if w.strip("'")]


def edits(ref, hyp):
    d = list(range(len(hyp) + 1))
    for i, r in enumerate(ref, 1):
        prev, d[0] = d[0], i
        for j, h in enumerate(hyp, 1):
            prev, d[j] = d[j], min(d[j] + 1, d[j - 1] + 1, prev + (r != h))
    return d[len(hyp)]


def load16k(path):
    """Mono float32 at 16 kHz from a 16-bit PCM WAV (band-limited FFT resampling)."""
    with wave.open(path) as w:
        ch, rate = w.getnchannels(), w.getframerate()
        x = np.frombuffer(w.readframes(w.getnframes()), dtype="<i2").astype(np.float64) / 32768.0
    x = x.reshape(-1, ch).mean(axis=1)
    n = int(round(len(x) * 16000 / rate))
    return (np.fft.irfft(np.fft.rfft(x)[: n // 2 + 1], n) * (n / len(x))).astype(np.float32)


def main():
    args = sys.argv[1:]
    against = None
    if "--against" in args:
        i = args.index("--against")
        against = args[i + 1]
        del args[i : i + 2]
    out = args[0]
    os.makedirs(out, exist_ok=True)
    sentences = [(line.rstrip("\n").split("\t") + ["train"])[:3] for line in open(os.path.join(HERE, "sentences.tsv"))]
    chimera = os.path.join(REPO, "target/release/chimera")
    jobs = [(kind, text, body, os.path.join(out, f"{kind}-{body}-{i + 1:02d}"), part)
            for i, (kind, text, part) in enumerate(sentences) for body in BODIES]

    def render(job):
        _, text, body, stem, _ = job
        subprocess.run([chimera, "render", "text", stem, text, body], check=True, capture_output=True)

    with ThreadPoolExecutor(os.cpu_count()) as pool:
        list(pool.map(render, jobs))

    model = WhisperModel("small.en", device="cpu", compute_type="int8", cpu_threads=os.cpu_count(),
                         download_root=CACHE)
    rows, totals = [], {}
    for kind, text, body, stem, part in jobs:
        segs, _ = model.transcribe(load16k(stem + ".wav"), language="en", beam_size=1, temperature=0.0,
                                   condition_on_previous_text=False, vad_filter=False)
        heard = " ".join(s.text.strip() for s in segs)
        ref = words(text)
        e = edits(ref, words(heard))
        rows.append({"kind": kind, "body": body, "set": part, "text": text, "heard": heard, "errors": e, "words": len(ref)})
        # "all" is the training sentences (as before held-out ones existed); "held" the others
        keys = (f"{kind}-{body}", "all") if part == "train" else (f"held-{kind}-{body}", "held")
        for key in keys:
            t = totals.setdefault(key, [0, 0])
            t[0] += e
            t[1] += len(ref)
    wer = {k: round(100.0 * e / n, 2) for k, (e, n) in totals.items()}
    report = {"wer": wer, "sentences": rows}
    with open(os.path.join(out, "wer.json"), "w") as f:
        json.dump(report, f, indent=1)
    # only the training sentences' transcripts: a tuning loop must not read the held-out ones
    for r in rows:
        if r["errors"] and r["set"] == "train":
            print(f"  {r['set']:5s} {r['kind']:8s} {r['body']:6s} {r['errors']}/{r['words']}  {r['text']}  →  {r['heard']}")
    print("word error rate (%): " + ", ".join(f"{k} {v}" for k, v in sorted(wer.items())))
    if against:
        before = json.load(open(against))["wer"]
        old, new = before["all"], wer["all"]
        verdict = "WORSE" if new > old + SLACK else "BETTER" if new <= old - MARGIN else "NO CLEAR CHANGE"
        held = f"; held-out {before['held']} → {wer['held']} %, not used to decide" if "held" in before else ""
        print(f"speech verdict against {against}: {verdict} (training word error rate {old} → {new} %, "
              f"better from -{MARGIN}, worse past +{SLACK}{held})")


main()
