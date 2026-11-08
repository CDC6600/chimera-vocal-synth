/**
 * The page's side of the plugin (docs/plugin.md, "The bridge"): commands to Rust through
 * `window.ipc`, and which store values are host parameters. Inside the plugin the sound lives in
 * Rust and this page is a view of it. On the website none of this runs.
 */
import type { ToWorklet } from "../worklet/protocol.ts";

declare global {
  interface Window {
    /** defined by wry in the plugin's webview, and by the test harness */
    ipc?: { postMessage(message: string): void };
  }
}

/** Whether this page is the plugin's editor. */
export const inPlugin = typeof window !== "undefined" && window.ipc !== undefined;

/** The host parameter behind each voice setting. Every other setting is saved with the project. */
export const SOURCE_PARAMS: Record<string, string> = { pressure: "breath", tension: "pressed", brightness: "bright", breathiness: "breathy", roughness: "rough", fry: "fry", glide: "glide", vibratoRate: "vibrato_rate", vibratoDepth: "vibrato" };
export const ARTIC_PARAMS: Record<string, string> = { epilarynx: "ring" };

/** The output gain parameter's range (dB), which the top bar's 0..1 slider spans. */
const GAIN_MIN = -30, GAIN_MAX = 12;
export const volumeToDb = (v: number) => GAIN_MIN + v * (GAIN_MAX - GAIN_MIN);
export const dbToVolume = (db: number) => (db - GAIN_MIN) / (GAIN_MAX - GAIN_MIN);

/** A gesture ends this long after its last value, so controls needn't know about hosts (ms). */
const GESTURE_IDLE = 300;
const gestures = new Map<string, ReturnType<typeof setTimeout>>();

/** The state revision this page last adopted: a command from before a restore is dropped there. */
let revision = -1;
export function setRevision(r: number): void {
  revision = r;
}

/** A command to Rust. */
export function ipc(message: Record<string, unknown>): void {
  window.ipc?.postMessage(JSON.stringify({ ...message, rev: revision }));
}

/** Move a host parameter (plain value), as one gesture until the moves stop. */
export function setParam(id: string, value: number | boolean): void {
  const open = gestures.get(id);
  if (open !== undefined) clearTimeout(open);
  gestures.set(
    id,
    setTimeout(() => {
      gestures.delete(id);
      ipc({ type: "paramEnd", id });
    }, GESTURE_IDLE),
  );
  ipc({ type: "param", id, value: typeof value === "boolean" ? (value ? 1 : 0) : value });
}

/** Whether this parameter is being moved here: a host value must not overwrite it. */
export function gestureOpen(id: string): boolean {
  return gestures.has(id);
}

/** The window's grip. Rust reads the pointer itself: the page's coordinates change with its zoom. */
export function grip(action: "begin" | "move" | "end" | "reset"): void {
  ipc({ type: "grip", action });
}

/** Split a message for the worklet into host parameters and what the project saves. */
export function sendToPlugin(m: ToWorklet): void {
  switch (m.type) {
    case "body":
      ipc({ type: "body", body: m.body });
      break;
    case "source": {
      const rest: Record<string, unknown> = {};
      for (const [k, v] of Object.entries(m.values)) {
        if (SOURCE_PARAMS[k]) setParam(SOURCE_PARAMS[k], v as number);
        else rest[k] = v;
      }
      if (Object.keys(rest).length) ipc({ type: "source", values: rest });
      break;
    }
    case "artic": {
      const rest: Record<string, unknown> = {};
      for (const [k, v] of Object.entries(m.values)) {
        if (ARTIC_PARAMS[k]) setParam(ARTIC_PARAMS[k], v as number);
        else rest[k] = v;
      }
      if (Object.keys(rest).length || m.tipFree !== undefined) ipc({ type: "artic", values: rest, tipFree: m.tipFree });
      break;
    }
    // the rest is what the page plans and plays by itself on the website: here Rust does it
    default:
      break;
  }
}
