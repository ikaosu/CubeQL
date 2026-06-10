/**
 * Message types shared between the renderer, main process, and worker thread.
 */

import type { OutputMode } from "../ui/cube-engine";

export interface SearchRequestMessage {
  type: "search-request";
  jobId: string;
  // Serializable inputs (typed arrays sent as plain arrays for structured-clone safety).
  initial: number[]; // 54 facelet colors
  mask: number[]; // 54 mask bytes
  dsl: string;
  maxMoves: number;
  outputMode: OutputMode;
  topN?: number;
}

export interface CancelMessage {
  type: "cancel";
  jobId: string;
}

export type ToWorker = SearchRequestMessage | CancelMessage;

export interface ProgressEvent {
  type: "progress";
  jobId: string;
  depth: number;
  solutionsFound: number;
  nodesExplored: number;
  elapsedMs: number;
}

export interface SolutionEvent {
  type: "solution";
  jobId: string;
  moves: number[];
  length: number;
}

export interface CompleteEvent {
  type: "complete";
  jobId: string;
  totalSolutions: number;
  timeMs: number;
  nodesExplored: number;
  shortestLength: number | null;
}

export interface ErrorEvent {
  type: "error";
  jobId: string;
  message: string;
}

/**
 * Reported as the worker loads / builds the pruning tables. Emitted on
 * worker startup before any search-request can be processed.
 */
export type TableBuildEvent =
  | { type: "table-build-start"; step: number; total: number; name: string; source: "cache" | "build" }
  | { type: "table-build-step-done"; step: number; total: number; name: string; elapsedMs: number }
  | { type: "table-build-all-done"; elapsedMs: number };

export type FromWorker =
  | ProgressEvent
  | SolutionEvent
  | CompleteEvent
  | ErrorEvent
  | TableBuildEvent;
