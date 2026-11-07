"""Check that each TSP recording says the sentence in its header: Whisper small.en (the speech
judge's model and settings) transcribes every recording in tools/tsp/sentences.tsv, and the
recordings with the most word errors are listed. Run it after regenerating the alignment.

usage: ~/.cache/chimera-asr/.venv/bin/python tools/tsp/check_text.py [tsp-dir] [out.json]

On 2026-11-07: 2.7 % word errors over the 1378 adult recordings (2.9 % female, 2.5 % male);
four recordings said another sentence and are excluded in align.py (EXCLUDE).
"""

import json
import os
import re
import sys

import numpy as np
from faster_whisper import WhisperModel

HERE = os.path.dirname(os.path.abspath(__file__))
CACHE = os.path.expanduser("~/.cache/chimera-asr/models")


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
    import wave
    with wave.open(path) as w:
        x = np.frombuffer(w.readframes(w.getnframes()), dtype="<i2").astype(np.float64) / 32768.0
        rate = w.getframerate()
    n = int(round(len(x) * 16000 / rate))
    return (np.fft.irfft(np.fft.rfft(x)[: n // 2 + 1], n) * (n / len(x))).astype(np.float32)


def main():
    tsp = sys.argv[1] if len(sys.argv) > 1 else os.path.expanduser("~/datasets/tsp")
    out = sys.argv[2] if len(sys.argv) > 2 else "out/tsp-text-check.json"
    rows = [l.split("\t") for l in open(os.path.join(HERE, "sentences.tsv")).read().splitlines()]
    model = WhisperModel("small.en", device="cpu", compute_type="int8", cpu_threads=os.cpu_count(),
                         download_root=CACHE)
    res, errs, n = {}, 0, 0
    for utt, spk, text in rows:
        segs, _ = model.transcribe(load16k(f"{tsp}/48k/{spk}/{utt}.wav"), language="en", beam_size=1,
                                   temperature=0.0, condition_on_previous_text=False, vad_filter=False)
        heard = " ".join(s.text.strip() for s in segs)
        e = edits(words(text), words(heard))
        res[utt] = {"text": text, "heard": heard, "errors": e}
        errs, n = errs + e, n + len(words(text))
    json.dump(res, open(out, "w"), indent=1)
    print(f"{100 * errs / n:.2f} % word errors over {len(rows)} recordings; worst:")
    for utt, r in sorted(res.items(), key=lambda kv: -kv[1]["errors"])[:15]:
        print(f"  {r['errors']}  {utt}  {r['text']}  |  {r['heard']}")


if __name__ == "__main__":
    main()
