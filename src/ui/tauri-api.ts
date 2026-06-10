// Tauri replacement for the Electron `window.api` surface.
//
// Renderer code (App.tsx etc.) still calls `window.api.*`. Under Electron the
// preload script populates that. Under Tauri there is no preload, so this
// module wires `window.api` to `@tauri-apps/api` (`invoke` + `event.listen`)
// using the IPC contract defined in `src-tauri/src/messages.rs`.

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { FromWorker, SearchRequestMessage } from "../shared/messages";

type ToWorkerLike = SearchRequestMessage | { type: "cancel"; jobId: string };

const WORKER_EVENT = "worker-event";

export const tauriApi = {
  sendSearch(msg: ToWorkerLike): void {
    if (msg.type !== "search-request") return;
    // Tauri commands receive their args under named keys; we use `req` to
    // match the `start_search(req: SearchRequest)` signature in Rust.
    void invoke("start_search", { req: msg }).catch((err: unknown) => {
      console.error("start_search invoke failed", err);
    });
  },
  cancelSearch(jobId: string): void {
    void invoke("cancel_search", { jobId }).catch((err: unknown) => {
      console.error("cancel_search invoke failed", err);
    });
  },
  onWorkerEvent(callback: (msg: FromWorker) => void): () => void {
    let unlisten: UnlistenFn | null = null;
    // `listen` is async — register synchronously and resolve later.
    void listen<FromWorker>(WORKER_EVENT, (e) => {
      callback(e.payload);
    }).then((fn) => {
      unlisten = fn;
    });
    return () => {
      if (unlisten) unlisten();
    };
  },
  async saveProject(defaultName: string, json: string): Promise<string | null> {
    return (await invoke("save_project", { defaultName, json })) as string | null;
  },
  async openProject(): Promise<{ path: string; data: string } | null> {
    return (await invoke("open_project")) as { path: string; data: string } | null;
  },
  // Snapshot of pruning-table build state. Lets the renderer recover from
  // events that fired before `listen()` was registered (cache-hit path
  // completes in ~110 ms — faster than React mount, so the banner would
  // otherwise stick on "loading pruning tables…" forever).
  async getPruningStatus(): Promise<{ ready: boolean; lastEvent: FromWorker | null }> {
    const s = (await invoke("get_pruning_status")) as {
      ready: boolean;
      lastEvent: FromWorker | null;
    };
    return s;
  },
  // Export solutions as a CSV file via native save dialog.
  async saveSolutionsCsv(defaultName: string, csv: string): Promise<string | null> {
    return (await invoke("save_solutions_csv", { defaultName, csv })) as string | null;
  },
  // Persisted UI settings (stored at `app_data_dir/settings.json`).
  async loadSettings(): Promise<string | null> {
    return (await invoke("load_settings")) as string | null;
  },
  async saveSettings(json: string): Promise<void> {
    await invoke("save_settings", { json });
  },
};

export type TauriApi = typeof tauriApi;

declare global {
  interface Window {
    api: TauriApi;
  }
}
