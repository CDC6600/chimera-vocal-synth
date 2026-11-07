"""Phone alignment of the TSP recordings, for `chimera compare tsp`. Run once by align.sh.

  align.py prepare <tsp-dir> <corpus-dir>   sentences.tsv, and an MFA corpus: one directory per
                                            utterance (MFA's "speaker"), with a wav link and .lab
  align.py collect <mfa-out-dir>            alignment.tsv.gz from MFA's JSON output

sentences.tsv: utterance, speaker, text (from each WAV's "text:" header record), adult speakers
only, less EXCLUDE. alignment.tsv: utterance, tier (word | phone), start (s), end (s), label (ARPAbet with
stress digits for phones, as MFA's english_us_arpa model gives them; silences dropped),
gzipped (mtime 0, so a rerun with the same result is byte-identical).
"""

import gzip
import json
import os
import re
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
# recordings whose audio is not the sentence in their header (found by check_text.py: Whisper
# hears the next sentence in FH46_01 and FH46_02, a misread take of FH46_04 in FH46_03, and
# "The fruit of a fig tree is apple-shaped." in FD21_01)
EXCLUDE = {"FD21_01", "FH46_01", "FH46_02", "FH46_03"}


def header_text(path):
    """The quoted "text:" record of a TSP WAV header (AFsp's afsp chunk)."""
    with open(path, "rb") as f:
        head = f.read(4096)
    m = re.search(rb'text: "([^"]*)"', head)
    if not m:
        raise SystemExit(f"{path}: no text record")
    return m.group(1).decode("latin-1")


def prepare(tsp, corpus):
    rows = []
    # absolute, so the links resolve from each utterance's directory
    root = os.path.join(os.path.abspath(tsp), "48k")
    for spk in sorted(os.listdir(root)):
        if not re.fullmatch(r"[FM][A-L]", spk):
            continue  # children CA, CB
        for name in sorted(os.listdir(os.path.join(root, spk))):
            if not name.endswith(".wav"):
                continue
            utt = name[:-4]
            if utt in EXCLUDE:
                continue
            src = os.path.join(root, spk, name)
            text = header_text(src)
            rows.append((utt, spk, text))
            os.makedirs(os.path.join(corpus, utt), exist_ok=True)
            link = os.path.join(corpus, utt, name)
            if not os.path.lexists(link):
                os.symlink(src, link)
            with open(os.path.join(corpus, utt, utt + ".lab"), "w") as f:
                f.write(text + "\n")
    with open(os.path.join(HERE, "sentences.tsv"), "w") as f:
        for r in rows:
            f.write("\t".join(r) + "\n")
    print(f"{len(rows)} utterances, {len({r[1] for r in rows})} speakers")


def collect(out):
    lines, missing = [], []
    utts = [r.split("\t")[0] for r in open(os.path.join(HERE, "sentences.tsv")).read().splitlines()]
    for utt in utts:
        path = os.path.join(out, utt, utt + ".json")
        if not os.path.exists(path):
            missing.append(utt)
            continue
        tiers = json.load(open(path))["tiers"]
        for tier, name in (("words", "word"), ("phones", "phone")):
            for start, end, label in tiers[tier]["entries"]:
                if label in ("", "sil", "sp", "<eps>"):
                    continue
                lines.append(f"{utt}\t{name}\t{start:.3f}\t{end:.3f}\t{label}")
    with open(os.path.join(HERE, "alignment.tsv.gz"), "wb") as raw:
        with gzip.GzipFile(fileobj=raw, mode="wb", mtime=0, filename="") as f:
            f.write(("\n".join(lines) + "\n").encode())
    print(f"{len(utts) - len(missing)} aligned, {len(missing)} missing: {' '.join(missing)}")


if __name__ == "__main__":
    cmd = sys.argv[1]
    if cmd == "prepare":
        prepare(sys.argv[2], sys.argv[3])
    elif cmd == "collect":
        collect(sys.argv[2])
    else:
        raise SystemExit(__doc__)
