# Render the bass-overtone example

This eight-second synthetic example uses a constant vocal-fold rate of 150 Hz.
It contains no recording, extracted melody, or lyrics.
It demonstrates source and tract controls. It is not a reconstruction of a Tuvan performance.

Run this command from the repository root:

```sh
cargo run --release -p chimera-cli -- render track examples/bass-overtone/track.json out/bass-overtone/voice.wav male --tele out/bass-overtone/voice.tele
```

Play `out/bass-overtone/voice.wav`. The track has these sections:

| Time | Controls |
| --- | --- |
| 0–2 seconds | Regular pulses at 150 Hz. |
| 2–4 seconds | Alternate pulses attenuated by 85 percent. The pulse pair repeats at 75 Hz. |
| 4–7 seconds | Independent tongue-tip and tract targets focus a harmonic near 1,500 Hz. |
| 7–8 seconds | Breath release. |

The tract produces the high tone from the source harmonics. The example adds no whistle oscillator or output equalizer.
The focused section has a lower output level because the narrow tract changes the acoustic load.

To use the existing animated show player, copy `voice.wav` and `voice.tele` to
`public/shows/bass-overtone/`. Create `show.json` in that folder:

```json
{
	"title": "Bass-overtone controls",
	"audio": "voice.wav",
	"voices": [{ "name": "CHIMERA", "tele": "voice.tele" }]
}
```

Run `bun run dev`. Open `http://127.0.0.1:5317/?show=bass-overtone` and press Play.

See [Control tracks](../../docs/control-tracks.md) for the format and model limits.
