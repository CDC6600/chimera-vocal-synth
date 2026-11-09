# Control tracks

`chimera render track` renders timed controls to a mono WAV at 48,000 Hz.
It accepts fractional pitch targets without conversion to musical notes.
The engine applies its existing pitch glide and articulator smoothing between targets.

```sh
chimera render track <in.json> <out> [male|female|child] [--tele voice.tele] [--float]
```

The default body is `male`. The output is 16-bit PCM unless `--float` selects 32-bit float.
`--tele` writes the same telemetry format as `render notes` for the animated show player.
Telemetry does not change the audio. Both outputs start at time zero.

## JSON format

```json
{
	"rate": 50,
	"source": { "glide": 0.015, "drift": 0 },
	"phonation": { "alternatingAttenuation": 0.85, "jitter": 0, "shimmer": 0 },
	"frames": [
		{ "f0": 148.5, "gate": 1, "tipFree": false, "artic": { "ttX": 5.8, "ttY": 8.5 } },
		{ "f0": 149.2, "gate": 0, "tipFree": false }
	]
}
```

| Field | Meaning |
| --- | --- |
| `rate` | Frames per second, from 1 through 1,000. Must divide 48,000 exactly. |
| `source` | Optional engine `SourcePatch`. Omitted fields retain the engine defaults. |
| `phonation` | Optional overrides for the cycle controls listed below. |
| `frames` | Nonempty array with a total duration of at most 600 seconds. |
| `frames[].f0` | Vocal-fold frequency, from 20 through 2,000 Hz. |
| `frames[].gate` | Breath gate, from 0 through 1.5. Zero releases the breath. |
| `frames[].artic` | Optional `ArticPatch`. Omitted articulator targets retain their previous values. |
| `frames[].tipFree` | Defaults to `true` in each frame. Set to `false` for independent tongue-tip targets. |
| `frames[].alternatingAttenuation` | Overrides the track value for this frame. Omission restores the track value or zero. |

Each frame lasts `1 / rate` seconds. The renderer adds no lead or tail.
The duration is exactly `frames.length / rate`, even if the final gate is open.
A track needs explicit release frames to include the end of a sound.

`tipFree: true` lets the existing articulator model place the tip from the tongue body and jaw.
`tipFree: false` makes `artic.ttX` and `artic.ttY` independent targets.
The engine already supports this control. The track command exposes it in offline renders.

The engine defines the patch fields in
[`SourcePatch`](../crates/chimera/src/voice/human_voice.rs) and
[`ArticPatch`](../crates/chimera/src/anatomy/human.rs).

## Phonation controls

| Field | Range | Effect |
| --- | --- | --- |
| `alternatingAttenuation` | 0 through 1 | Suppresses this fraction of every second glottal area pulse. Defaults to zero. |
| `diplophonia` | 0 through 1 | Alternates both pulse amplitude and period. |
| `jitter` | 0 through 0.1 | Sets random period variation. |
| `shimmer` | 0 through 1 | Sets random amplitude variation. |

The renderer reapplies these overrides after each pitch update because `set_source` derives cycle controls from source roughness.
Omitted controls retain the source-derived values.

Alternate-pulse attenuation preserves the vocal-fold period. At a fold rate of 150 Hz, the pulse pair repeats at 75 Hz.
This approximates pulse suppression in bass throat singing. It does not model a separate ventricular-fold oscillator or its pressure coupling.
The attenuation acts on glottal area, so it is not a calibrated measure of airflow attenuation.
Its zero default preserves the existing voice presets.
