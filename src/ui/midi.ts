/**
 * Web MIDI keyboards: note on/off (with velocity) from every connected input, including ones
 * plugged in later. The voice hears one note per pitch: a pitch held on several keyboards or
 * channels sounds until the last of them lets go. All-notes-off and unplugging release what
 * that input (channel) was holding.
 */
import { inPlugin } from "../audio/pluginBridge.ts";

export type MidiStatus = "off" | "connecting" | "on" | "unsupported" | "denied";

export interface MidiHandlers {
  noteOn: (note: number, velocity: number) => void;
  noteOff: (note: number) => void;
  /** names of the connected inputs (on every change) */
  inputs: (names: string[]) => void;
}

/** The plugin gets its MIDI from the host. */
export const midiSupported = (): boolean => !inPlugin && typeof navigator !== "undefined" && typeof navigator.requestMIDIAccess === "function";

let handlers: MidiHandlers | null = null;
let access: MIDIAccess | null = null;
let pending: Promise<void> | null = null;
/** bumped by disconnect: a connection still waiting for permission then gives up */
let generation = 0;
/** who holds each pitch, as "input id \0 channel" */
const owners = new Map<number, Set<string>>();
const SEP = "\u0000";

function press(owner: string, note: number, velocity: number): void {
  let set = owners.get(note);
  if (!set) owners.set(note, (set = new Set()));
  const first = set.size === 0;
  set.add(owner);
  if (first) handlers?.noteOn(note, velocity);
}

function lift(owner: string, note: number): void {
  const set = owners.get(note);
  if (!set || !set.delete(owner)) return;
  if (set.size > 0) return;
  owners.delete(note);
  handlers?.noteOff(note);
}

/** Release every note whose owner matches. */
function liftWhere(match: (owner: string) => boolean): void {
  for (const [note, set] of [...owners]) for (const o of [...set]) if (match(o)) lift(o, note);
}

function attach(a: MIDIAccess): void {
  const names: string[] = [];
  const live = new Set<string>();
  a.inputs.forEach((input) => {
    if (input.state === "disconnected") return;
    live.add(input.id);
    names.push(input.name || "MIDI keyboard");
    input.onmidimessage = (e: MIDIMessageEvent) => {
      const d = e.data;
      if (!d || d.length < 2) return;
      const kind = d[0] & 0xf0;
      const owner = input.id + SEP + (d[0] & 0x0f);
      if (kind === 0x90 && d.length > 2 && d[2] > 0) press(owner, d[1], d[2]);
      else if (kind === 0x80 || kind === 0x90) lift(owner, d[1]);
      else if (kind === 0xb0 && (d[1] === 123 || d[1] === 120)) liftWhere((o) => o === owner); // all notes / sound off
    };
  });
  // unplugged inputs release what they held
  liftWhere((o) => !live.has(o.slice(0, o.indexOf(SEP))));
  handlers?.inputs(names);
}

/** Ask for MIDI access and listen (one request at a time; resolves when connected). */
export function connectMidi(h: MidiHandlers): Promise<void> {
  handlers = h;
  if (access) return Promise.resolve();
  if (pending) return pending;
  const gen = generation;
  const p = navigator.requestMIDIAccess({ sysex: false }).then(
    (a) => {
      if (gen !== generation) return; // disconnected while the browser was asking
      access = a;
      a.onstatechange = () => attach(a);
      attach(a);
    },
  );
  pending = p;
  const clear = () => {
    if (pending === p) pending = null;
  };
  p.then(clear, clear);
  return p;
}

export function disconnectMidi(): void {
  generation++;
  pending = null;
  if (access) {
    access.inputs.forEach((input) => (input.onmidimessage = null));
    access.onstatechange = null;
    access = null;
  }
  liftWhere(() => true);
}
