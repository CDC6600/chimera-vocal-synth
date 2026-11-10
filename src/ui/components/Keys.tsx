import { useEffect, useRef, useState } from "react";
import { useStore } from "../store.ts";
import { midiSupported } from "../midi.ts";
import { nearestBody, type BodyMix } from "../body.ts";

/** Computer-keyboard layout: two rows like a piano (A W S E D F T G Y H U J K O L P ;). */
const KEYMAP: Record<string, number> = { a: 0, w: 1, s: 2, e: 3, d: 4, f: 5, t: 6, g: 7, y: 8, h: 9, u: 10, j: 11, k: 12, o: 13, l: 14, p: 15, ";": 16 };
const BASES: Record<string, number> = { male: 48, female: 57, child: 60 };
/** the lowest key, from the nearest preset body (so the keys stay on whole octaves of C/A) */
const BASE = (b: BodyMix) => BASES[nearestBody(b)];

export function Keys() {
  const body = useStore((s) => s.body);
  const held = useStore((s) => s.heldNotes);
  const noteOn = useStore((s) => s.noteOn);
  const noteOff = useStore((s) => s.noteOff);
  const start = useStore((s) => s.start);
  // the key handler reads the ref (it is set up once); the state redraws the keys on Z and X
  const octave = useRef(0);
  const [shownOctave, setShownOctave] = useState(0);
  const base = BASE(body) + shownOctave * 12;
  /**
   * on-screen key held by a pointer → the note it played, released when that pointer lifts
   * anywhere: Z, X or a body change can redraw the keys under a held finger and drop its button
   */
  const byPointer = useRef(new Map<number, number>());

  useEffect(() => {
    /** pressed key → the note it played (released as that note, whatever the body or octave now) */
    const down = new Map<string, number>();
    const onDown = (e: KeyboardEvent) => {
      if (e.repeat || e.metaKey || e.ctrlKey || e.altKey) return;
      const target = e.target as HTMLElement;
      if (target && (target.tagName === "INPUT" || target.tagName === "TEXTAREA" || target.isContentEditable)) {
        if ((target as HTMLInputElement).type !== "range" && (target as HTMLInputElement).type !== "checkbox") return;
      }
      const k = e.key.toLowerCase();
      if (k === "z" || k === "x") {
        octave.current = Math.max(-2, Math.min(2, octave.current + (k === "z" ? -1 : 1)));
        setShownOctave(octave.current);
      }
      if (!(k in KEYMAP) || down.has(k)) return;
      const midi = BASE(useStore.getState().body) + octave.current * 12 + KEYMAP[k];
      down.set(k, midi);
      void start();
      noteOn(midi);
      e.preventDefault();
    };
    const onUp = (e: KeyboardEvent) => {
      const k = e.key.toLowerCase();
      const midi = down.get(k);
      if (midi === undefined) return;
      down.delete(k);
      noteOff(midi);
    };
    const onBlur = () => {
      for (const midi of down.values()) noteOff(midi);
      down.clear();
    };
    const onPointerUp = (e: PointerEvent) => {
      const midi = byPointer.current.get(e.pointerId);
      if (midi === undefined) return;
      byPointer.current.delete(e.pointerId);
      noteOff(midi);
    };
    window.addEventListener("keydown", onDown);
    window.addEventListener("keyup", onUp);
    window.addEventListener("blur", onBlur);
    window.addEventListener("pointerup", onPointerUp);
    window.addEventListener("pointercancel", onPointerUp);
    return () => {
      window.removeEventListener("keydown", onDown);
      window.removeEventListener("keyup", onUp);
      window.removeEventListener("blur", onBlur);
      window.removeEventListener("pointerup", onPointerUp);
      window.removeEventListener("pointercancel", onPointerUp);
    };
  }, [noteOn, noteOff, start]);

  const notes = Array.from({ length: 25 }, (_, i) => base + i);
  const isBlack = (m: number) => [1, 3, 6, 8, 10].includes(((m % 12) + 12) % 12);
  const whites = notes.filter((m) => !isBlack(m));
  const press = (m: number) => (e: React.PointerEvent) => {
    (e.target as Element).setPointerCapture(e.pointerId);
    byPointer.current.set(e.pointerId, m);
    void start();
    noteOn(m);
  };
  return (
    <div className="keys-row">
      <MidiButton />
      <div className="keys" data-layout="layered" aria-label="Keyboard. Computer keys A to semicolon play notes; Z and X change octave.">
        <div className="white-row">
          {whites.map((m) => (
            <button key={m} className={`key white${held.includes(m) ? " down" : ""}`} onPointerDown={press(m)} aria-label={`Note ${m}`} />
          ))}
        </div>
        <div className="black-row">
          {whites.map((m, i) => {
            const b = m + 1;
            const hasBlack = isBlack(b) && i < whites.length - 1;
            return hasBlack ? (
              <button key={b} className={`key black${held.includes(b) ? " down" : ""}`} style={{ left: `calc(${((i + 1) / whites.length) * 100}% - 1.1%)` }} onPointerDown={press(b)} aria-label={`Note ${b}`} />
            ) : null;
          })}
        </div>
      </div>
    </div>
  );
}

/** Connect a MIDI keyboard (any mode with keys); shows what is connected. */
function MidiButton() {
  const midi = useStore((s) => s.midi);
  const setMidi = useStore((s) => s.setMidi);
  if (!midiSupported()) return null;
  const on = midi.status === "on";
  const status =
    midi.status === "connecting"
      ? "Connecting…"
      : midi.status === "denied"
        ? "Blocked by the browser"
        : on
          ? midi.inputs.length
            ? midi.inputs.join(", ")
            : "Plug one in"
          : "Off";
  return (
    <button className={`midi${on ? " on" : ""}`} aria-pressed={on} onClick={() => setMidi(!on)} title={on ? "Disconnect the MIDI keyboard" : "Play with a MIDI keyboard (the browser asks for access)"}>
      <span className="midi-name">MIDI keyboard</span>
      <span className="midi-status">{status}</span>
    </button>
  );
}
