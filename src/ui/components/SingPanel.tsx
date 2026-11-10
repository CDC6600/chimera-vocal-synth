import { useEffect } from "react";
import { useStore } from "../store.ts";
import { useSing, playDemo, stopSinging, soundingSyllable, sungNow, DEMO_SONGS, ensureDictionary } from "../singing.ts";
import { mouthWords } from "../mouthWords.ts";
import { V } from "../colors.ts";
import { Keys } from "./Keys.tsx";
import { PianoRoll } from "./PianoRoll.tsx";
import { SpectrumView } from "./SpectrumView.tsx";
import { Chip, Screen } from "./Screen.tsx";
import { EncoderPages } from "./Encoder.tsx";
import { noteName } from "./VoiceControls.tsx";
import type { InfoFn } from "./AnatomyView.tsx";

/** epilarynx narrowing for the singer's ring (measured: +3 to +12 dB at 2.4–3.6 kHz, adult male) */
const RING = 0.85;

/** the syllable being sung now: the demo's, or the one the held key is singing */
function syllableNow(): string | null {
  const s = useSing.getState();
  if (s.demo) return s.syllables[s.demoAt]?.text ?? null;
  return soundingSyllable();
}

/** the syllable being sung, at the mouth */
export const singWords = mouthWords(sungNow, V.green);

/** Sing mode's lines over the face: the note, the syllable, the ring */
export const singInfo: InfoFn = (_m, _f, tele) => {
  const out: string[] = [];
  if (tele && tele.t.voicing > 0.3) out.push(`Note ${noteName(tele.t.f0)} ${tele.t.f0.toFixed(0)} Hz`);
  const syl = syllableNow();
  if (syl) out.push(`Syllable ${syl}`);
  out.push(useStore.getState().artic.epilarynx >= RING - 0.05 ? "Ring on" : "Ring off");
  return out;
};

function Lyrics() {
  const lyrics = useSing((s) => s.lyrics);
  const syllables = useSing((s) => s.syllables);
  const cursor = useSing((s) => s.cursor);
  const demo = useSing((s) => s.demo);
  const demoAt = useSing((s) => s.demoAt);
  const setLyrics = useSing((s) => s.setLyrics);
  const setCursor = useSing((s) => s.setCursor);
  const at = demo ? demoAt : -1;
  const done = demo ? demoAt : cursor;
  return (
    <div className="lyrics">
      <textarea
        value={lyrics}
        onChange={(e) => setLyrics(e.target.value)}
        onKeyDown={(e) => e.stopPropagation()}
        rows={1}
        aria-label="Lyrics to sing"
        placeholder="Type the words to sing"
        title="Each note sings the next syllable. Hold a key while you press the next one to sing legato. A lone “-” between words holds a vowel over several notes; hyphens split a word your way (“ti-ger”)."
      />
      <div className="syllables" role="group" aria-label="Syllables. Each note sings the marked one; choose another to start there.">
        {syllables.map((s, i) => (
          <span key={i} className={`syl-wrap${s.wordEnd ? " word-end" : ""}`}>
            <button
              className={`syl${!demo && i === cursor ? " next" : ""}${i === at ? " now" : ""}${i < done ? " sung" : ""}${s.melisma ? " held" : ""}`}
              onClick={() => setCursor(i)}
              aria-current={!demo && i === cursor ? "step" : undefined}
              title={s.melisma ? "Held vowel: carries the previous syllable onto this note" : `Sing from “${s.text}”`}
            >
              {s.text}
            </button>
            {s.phraseEnd && (
              <span className="breath-mark" aria-label="breath">
                ’
              </span>
            )}
          </span>
        ))}
        {!syllables.length && <span className="dim">No words: the keys sing “ah”.</span>}
        <Chip toggle={false} className="from-start" onClick={() => setCursor(0)} disabled={cursor === 0 && !demo}>
          From the start
        </Chip>
      </div>
    </div>
  );
}

function SongControls() {
  const source = useStore((s) => s.source);
  const setSource = useStore((s) => s.setSource);
  const epilarynx = useStore((s) => s.artic.epilarynx);
  const setArtic = useStore((s) => s.setArtic);
  const vibrato = useSing((s) => s.vibrato);
  const setVibrato = useSing((s) => s.setVibrato);
  return (
    <EncoderPages
      label="Song voice"
      pages={[
        {
          name: "Song",
          encoders: [
            { label: "Vibrato", value: vibrato, min: 0, max: 80, onChange: (v) => setVibrato(Math.round(v)), format: (v) => ({ value: String(Math.round(v)), unit: "cents" }) },
            { label: "Rate", value: source.vibratoRate, min: 3.5, max: 7.5, onChange: (v) => setSource({ vibratoRate: v }), format: (v) => ({ value: v.toFixed(1), unit: "Hz" }) },
            { label: "Breathy", value: source.breathiness, min: 0, max: 1, onChange: (v) => setSource({ breathiness: v }), format: (v) => ({ value: String(Math.round(v * 100)), unit: "%" }) },
            // the singer's ring narrows the tube just above the folds, adding the bright band near 3 kHz
            { label: "Ring", value: epilarynx, min: 0, max: 1, onChange: (v) => setArtic({ epilarynx: v }), format: (v) => ({ value: String(Math.round(v * 100)), unit: "%" }) },
          ],
        },
        {
          name: "Tone",
          encoders: [{ label: "Bright", value: source.brightness, min: 0, max: 1, onChange: (v) => setSource({ brightness: v }), format: (v) => ({ value: String(Math.round(v * 100)), unit: "%" }) }],
        },
      ]}
    />
  );
}

function DemoSongs() {
  const body = useStore((s) => s.body);
  const start = useStore((s) => s.start);
  const demo = useSing((s) => s.demo);
  return (
    <div className="demo-songs" role="group" aria-label="Demo songs">
      {DEMO_SONGS.map((s) => (
        <Chip
          key={s.name}
          on={demo === s.name}
          onClick={async () => {
            if (demo === s.name) {
              stopSinging();
              return;
            }
            await playDemo(s.name, body, start);
          }}
        >
          {demo === s.name ? `Stop “${s.name}”` : s.name}
        </Chip>
      ))}
    </div>
  );
}

/** Sing mode's right column: the piano roll, the lyrics, the spectrum with the ring, the song voice, the keys. */
export function SingRight() {
  // the pronouncing dictionary for the lyrics: here rather than on the mode switch, since the app
  // opens on Sing without one
  useEffect(ensureDictionary, []);
  const ring = useStore((s) => s.artic.epilarynx >= RING - 0.05);
  const next = useSing((s) => (s.demo ? null : (s.syllables[s.cursor]?.text ?? null)));
  return (
    <div className="right right-sing">
      <Screen className="s-roll" title="Piano roll" sub="Notes and the pitch sung">
        <PianoRoll />
      </Screen>
      <Screen className="s-lyrics" title="Lyrics" sub="Each note sings the next syllable" tools={<DemoSongs />}>
        <Lyrics />
      </Screen>
      <Screen className="s-spec" title="Spectrum" sub={ring ? "Ring on" : ""}>
        <SpectrumView ring={ring} />
      </Screen>
      <Screen className="s-voice" title="Voice">
        <SongControls />
      </Screen>
      <Screen className="s-keys" title="Keys" sub={next ? `Next: ${next}` : ""}>
        <Keys />
      </Screen>
    </div>
  );
}
