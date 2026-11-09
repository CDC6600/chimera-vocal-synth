# Make a throat-singing demo

Extract a vocal passage, estimate its pitch and vowel shapes, and render those controls through
CHIMERA. The engine generates the voice. Demucs separates the recorded accompaniment for the mix.

This is an approximation. The source can suppress alternate vocal-fold area pulses, but its
ventricular folds provide static narrowing only. The analysis estimates vowels from the spectral
envelope. It does not transcribe lyrics or reconstruct consonants. Source separation can leave
some of the original voice in the accompaniment.

## Render the voice

Run these commands from the repository root. Use a recording that you have locally.
The example selects seconds 27 through 82.

```sh
uv venv --python 3.12 out/throat/.venv
uv pip install --python out/throat/.venv/bin/python numpy scipy soundfile 'demucs==4.1.0' 'torch==2.8.0' 'torchaudio==2.8.0'
ffmpeg -y -ss 27 -i recording.mp3 -t 55 -ar 44100 -c:a pcm_f32le out/throat/excerpt.wav
OMP_NUM_THREADS=4 out/throat/.venv/bin/python -m demucs.separate -n htdemucs -d cpu --shifts 0 --segment 7 --two-stems vocals --other-method minus --float32 --clip-mode none -o out/throat/separated out/throat/excerpt.wav
out/throat/.venv/bin/python tools/throat-track.py out/throat/separated/htdemucs/excerpt/vocals.wav out/throat/track.json
cargo run --release -p chimera-cli -- render track out/throat/track.json out/throat/chimera-dry.wav male --tele out/throat/chimera.tele --float
```

The first separation run downloads the model. Inference runs locally. Demucs writes `vocals.wav`
and `minus_vocals.wav`. The two files share the excerpt's timeline.

The analyzer estimates controls at 50 Hz and interpolates them to 200 frames per second.
Its default pitch search covers 45–140 Hz.
Use `--fmin` and `--fmax` for another register. The default `--fold-ratio 2` drives the folds at
twice the measured repetition rate. The source suppresses alternate pulses to produce the lower component.
Use `--fold-ratio 1` to drive them at the measured rate. Pitch tracking can confuse octaves in a
rough voice. The analyzer selects a continuous path through candidate pitches to reduce these errors.
Breath follows the audio level independently of pitch confidence. Mouth shapes blend between vowel
postures instead of selecting one posture per frame. Compare the isolated vocal track with the render
before choosing a setting.

## Focus the overtones

Fit tract postures, then track the high spectral focus in the original mix. The original is
necessary because a vocal separator can put whistle-like vocal harmonics in the accompaniment.
The fit changes tongue, lip, jaw, and pharynx positions. The voice engine generates the result
through its airway. It does not add a whistle oscillator or apply an equalizer to the output.

```sh
cargo run --release -p chimera --example overtone -- 950 900 $(seq 1000 50 3300) > out/throat/focus-postures.json
out/throat/.venv/bin/python tools/focus-throat.py out/throat/track.json out/throat/excerpt.wav out/throat/focus-postures.json out/throat/focused-track.json
cargo run --release -p chimera-cli -- render track out/throat/focused-track.json out/throat/chimera-dry.wav male --tele out/throat/chimera.tele --float
```

The tracker looks for a narrow focus above 1.6 kHz. During sustained low-confidence regions in
the separated vocal track, it also checks 0.9–1.6 kHz with stricter concentration thresholds.
It holds the last reliable low pitch when it restores a focused passage. The lower-focus mode
reduces alternate-pulse attenuation to approximate a change of vocal technique.
These are acoustic estimates, not verified labels of the singer's technique. Bowed-instrument
harmonics can still affect the estimates. No words or interjections are transcribed.

The fit rejects near-closed average tube areas and penalizes competing high resonances.
Its frequency-domain response omits mean-flow losses, so check a rendered tone as well as the
fitted response. `render_track_can_focus_a_high_harmonic_while_retaining_the_low_voice` checks
both the high harmonic and the continuing low voice in rendered audio.

## Play the animated head

Create `public/shows/throat-singing/`. Copy `chimera.tele` there as `voice.tele`.
Put the rendered audio there as `mix.wav`, or mix it with `minus_vocals.wav` first.
Match their sample rates and adjust their levels before mixing. Preserve sample zero and duration.
Create `show.json` in the same folder:

```json
{
  "title": "Throat singing experiment",
  "audio": "mix.wav",
  "voices": [{ "name": "CHIMERA", "tele": "voice.tele" }]
}
```

Run `bun run dev`. Open `http://127.0.0.1:5317/?show=throat-singing` and press Play.
Keep the source recording, stems, extracted controls, and renders in the ignored `out/` and
`public/shows/` folders.

## Model limits

See [Control tracks](control-tracks.md) for the JSON format that `throat-track.py` and
`focus-throat.py` write. The analyzer uses `alternatingAttenuation: 0.85`, `diplophonia: 0`,
`jitter: 0.003`, and `shimmer: 0.02`. These are experimental values, not measurements of a
singer's anatomy.

Two studies guide the model:

- [Lindestad et al., 2001](https://doi.org/10.1016/S0892-1997(01)00008-X) used high-speed imaging
  of bass throat singing. The ventricular folds closed at half the true-fold rate and reduced
  alternate airflow pulses. The engine approximates this effect when it attenuates alternate
  glottal area pulses and keeps their timing. It does not model the second tissue oscillator or its
  pressure coupling. Area attenuation is not a calibrated measure of airflow attenuation.
- [Bergevin et al., 2020](https://elifesciences.org/articles/50476) studied Sygyt overtone focusing.
  Tongue and pharyngeal constrictions bring two vocal-tract resonances together. The high tone can
  change while the underlying pulse rate stays stable. The analyzer therefore estimates the pitch
  path separately from the changing resonances. Its vowel blends approximate the tract.

A recording alone does not establish a performer's fold motion. The tools test a bass-style source
approximation. They do not reproduce every throat-singing technique.

## Test the extractor

Run the extraction regressions in the same Python environment:

```sh
PYTHONDONTWRITEBYTECODE=1 out/throat/.venv/bin/python -m unittest discover -s tools -p 'test_throat_track.py' -v
```
