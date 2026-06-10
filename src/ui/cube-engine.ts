// Renderer-side cube engine. SINGLE SOURCE OF TRUTH = cube-core WASM build.
//
// All facelet / move / mask / DSL / preset / project logic is implemented
// once in Rust (`cube-core` crate). The renderer never duplicates that
// logic in TS — anything beyond the constants below must come through the
// WASM bindings exported from `../../cube-core-wasm/`.

import init, * as wasm from "../../cube-core-wasm/cube_core";

// ----- Constants (pure data, no Rust call needed) -----

export const FACELET_COUNT = 54;

export const Color = { U: 0, R: 1, F: 2, D: 3, L: 4, B: 5 } as const;
export type Color = (typeof Color)[keyof typeof Color];

export const COLOR_NAMES = ["U", "R", "F", "D", "L", "B"] as const;

export type Mask = Uint8Array;

export type OutputMode = "shortest-only" | "enumerate-up-to-N" | "top-n";

// ----- Init -----

let _ready = false;
let _readyPromise: Promise<void> | null = null;
/**
 * Load the WASM module. Must complete before any of the wrapped functions
 * below are called. `main.tsx` awaits this before mounting React.
 */
export function initEngine(): Promise<void> {
  if (_ready) return Promise.resolve();
  if (!_readyPromise) {
    _readyPromise = init().then(() => {
      _ready = true;
    });
  }
  return _readyPromise;
}

// ----- Re-exported WASM functions -----
//
// These are bound to the WASM instance lazily — calling them before
// `initEngine()` resolves will throw. Renderer code reaches them via the
// imports below; no other module should import from `cube-core-wasm`
// directly so this file stays the single boundary.

export const createSolvedFacelet = (): Int8Array => wasm.createSolvedFacelet();
export const createFullMask = (): Uint8Array => wasm.createFullMask();
export const createEmptyMask = (): Uint8Array => wasm.createEmptyMask();

export const validateFacelet = (facelet: Int8Array, mask?: Uint8Array): string | undefined =>
  wasm.validateFacelet(facelet, mask);

export const parseMoveToken = (token: string): number | null => {
  const v = wasm.parseMoveToken(token);
  return v === undefined ? null : v;
};

export const applyMoves = (facelet: Int8Array, moves: readonly number[]): Int8Array =>
  wasm.applyMoves(facelet, Int32Array.from(moves));

export const applyMovesToMask = (mask: Uint8Array, moves: readonly number[]): Uint8Array =>
  wasm.applyMovesToMask(mask, Int32Array.from(moves));

export const formatMoveSequence = (moves: readonly number[]): string =>
  wasm.formatMoveSequence(Int32Array.from(moves));

// ----- Presets -----

export interface PresetInfo {
  id: string;
  label: string;
  description: string;
}

let _presetsCache: PresetInfo[] | null = null;
export function getPresets(): PresetInfo[] {
  if (!_presetsCache) {
    _presetsCache = wasm.presetList() as PresetInfo[];
  }
  return _presetsCache;
}

export function presetById(id: string): PresetInfo | null {
  const p = wasm.presetById(id) as PresetInfo | null;
  return p;
}

export function maskFromPresetId(id: string): Uint8Array | null {
  const m = wasm.maskFromPresetId(id);
  return m === undefined ? null : m;
}

// ----- DSL (parse + format) -----

export interface DslOk {
  ok: true;
  preview: string;
}
export interface DslErr {
  ok: false;
  error: { message: string; offset: number; line: number; column: number };
}
export type DslState = DslOk | DslErr;

/** Parse a CubeQL pattern, returning a flat success/error record. */
export function parsePatternPreview(src: string): DslState {
  const r = wasm.parsePatternPreview(src) as
    | { ok: true; preview: string }
    | { ok: false; message: string; offset: number; line: number; column: number };
  if (r.ok) return { ok: true, preview: r.preview };
  return {
    ok: false,
    error: { message: r.message, offset: r.offset, line: r.line, column: r.column },
  };
}

// ----- Project (.gencube.json) -----

export interface ProjectData {
  facelet: Int8Array;
  mask: Uint8Array;
  preset: string | null;
  dsl: string;
  maxMoves: number;
  outputMode: OutputMode;
  topN: number | null;
}

export function parseProject(json: string): ProjectData {
  const w = wasm.parseProjectJson(json) as {
    facelet: number[];
    mask: number[];
    preset: string | null;
    dsl: string;
    maxMoves: number;
    outputMode: OutputMode;
    topN: number | null;
  };
  return {
    facelet: Int8Array.from(w.facelet),
    mask: Uint8Array.from(w.mask),
    preset: w.preset,
    dsl: w.dsl,
    maxMoves: w.maxMoves,
    outputMode: w.outputMode,
    topN: w.topN,
  };
}

export interface BuildProjectInput {
  facelet: Int8Array;
  mask: Uint8Array;
  preset: string | null;
  dsl: string;
  maxMoves: number;
  outputMode: OutputMode;
  topN?: number;
}

export function buildProjectJson(input: BuildProjectInput): string {
  return wasm.buildProjectJson(
    input.facelet,
    input.mask,
    input.preset ?? undefined,
    input.dsl,
    input.maxMoves,
    input.outputMode,
    input.topN ?? undefined,
  );
}

export function faceletFromString(s: string): Int8Array {
  return wasm.faceletFromString(s);
}
