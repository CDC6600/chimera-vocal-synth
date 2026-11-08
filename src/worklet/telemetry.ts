/**
 * The human voice's flat telemetry block (`VoiceTelemetry::write_flat` in
 * crates/chimera/src/voice/human_voice.rs), as the UI reads it. Shared by the AudioWorklet and the
 * plugin bridge, so it must not use anything the AudioWorkletGlobalScope lacks.
 */
import { fromUtf8 } from "../wasm/wire.ts";
import type { VoiceTelemetry } from "../wasm/types.ts";

/** scalars at the head of the block */
export const TELE_HEADER = 16;
/** bytes of the active-label area: up to 4 × (length byte, 24 bytes) */
export const TELE_LABELS = 4 * 25;

/** The block's whole length, from its header. */
export function telemetryLength(head: Float64Array): number {
  const [nArtic, nPh, nOr, nScope] = [head[11], head[12], head[13], head[14]];
  return TELE_HEADER + nArtic + 3 * (nPh + nOr) + 2 * nScope;
}

/** The block as the UI reads it. The arrays are copies, so the block may be overwritten after. */
export function decodeTelemetry(block: Float64Array, labels: Uint8Array): { t: VoiceTelemetry; nPh: number; nOr: number } {
  const [phLen, orLen, velumPort, f0, voicing, pLung, outRms, noiseLevel, noiseNode, gate, clock, nArtic, nPh, nOr, nScope, nActive] = block;
  let o = TELE_HEADER;
  const take = (n: number) => Float32Array.from(block.subarray(o, (o += n)));
  const artic = take(nArtic), phAreas = take(nPh), orAreas = take(nOr), pressureRms = take(nPh + nOr), flow = take(nPh + nOr), glottalArea = take(nScope), glottalFlow = take(nScope);
  const active: string[] = [];
  for (let i = 0; i < nActive; i++) active.push(fromUtf8(labels.subarray(i * 25 + 1, i * 25 + 1 + labels[i * 25])));
  return { t: { artic, phAreas, orAreas, phLen, orLen, pressureRms, flow, velumPort, glottalArea, glottalFlow, f0, voicing, pLung, outRms, noiseLevel, noiseNode, gate, clock, active }, nPh, nOr };
}
