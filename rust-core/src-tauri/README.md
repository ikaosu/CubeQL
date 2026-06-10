# src-tauri

Tauri 2.x desktop shell for gen-search. Wires the React renderer (`../../src/ui/`)
to `cube-core` via the IPC commands declared in [src/lib.rs](src/lib.rs).

## Layout

```
src-tauri/
├── Cargo.toml              workspace member; depends on cube-core
├── tauri.conf.json         identifier dev.gencube.app, window 1400x900
├── build.rs                tauri-build trampoline
├── capabilities/default.json   permissions: core + dialog + fs
├── icons/                  app icons (Windows/macOS/Linux)
└── src/
    ├── main.rs             bin entry — calls gen_search_tauri_lib::run()
    ├── lib.rs              IPC handlers + Tauri Builder
    └── messages.rs         serde types mirroring ../../src/shared/messages.ts
```

## IPC contract

Commands (renderer → Rust):

| command         | payload                              | returns                            |
|-----------------|--------------------------------------|------------------------------------|
| `start_search`  | `{ req: SearchRequest }`             | `{}` (events stream over channel)  |
| `cancel_search` | `{ jobId: string }`                  | `{}` (also emits synthetic `complete`) |
| `save_project`  | `{ defaultName: string, json: string }` | `string \| null` (chosen path)  |
| `open_project`  | `{}`                                  | `{ path, data } \| null`          |

Event channel (Rust → renderer): `worker-event`, payload is a tagged union of
`progress` / `solution` / `complete` / `error` / `table-build-{start,step-done,all-done}`
matching `FromWorker` in `src/shared/messages.ts`.

## Pruning-table cache

`Builder::setup` pins `GENCUBE_PRUNING_CACHE` to `app_data_dir/pruning-cache-v1/`
so the 88 MB Corner PDB builds once per install. Format is byte-compatible with
the Electron TS implementation, so an existing `%TEMP%/gen-search-pruning-v1/`
cache can be moved into the app data dir to avoid the one-time ~3-minute build.

## Dev / build

The renderer is built by `../../vite.tauri.config.ts` (outputs to `dist-tauri/`).
Tauri's `beforeDevCommand` / `beforeBuildCommand` call `npm --prefix ../.. run
tauri-dev` / `tauri-build` respectively.

```powershell
# from rust-core/
cargo tauri dev      # spawns vite on :1420 + opens window
cargo tauri build    # produces MSI / app / AppImage under target/release/bundle/
```

## Job cancellation

`JobRegistry` (managed Tauri state) holds an `Arc<AtomicBool>` per in-flight
`jobId`. `start_search` registers a fresh flag and passes
`move || flag.load(Relaxed)` as the engine's `should_cancel` callback;
`cancel_search` looks up the flag and sets it. The search thread polls
periodically (per DFS node, in `engine.rs`).
