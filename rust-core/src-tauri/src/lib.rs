//! Tauri 2.x entry — wires the React renderer to `cube-core`.
//!
//! IPC commands replace the Electron `window.api.*` surface:
//!   start_search / cancel_search / save_project / open_project
//!
//! Events emitted to the renderer via `worker-event`:
//!   progress / solution / complete / error / table-build-{start,step-done,all-done}
//!
//! Pruning-table cache lives under `app_data_dir/pruning-cache-<CACHE_VERSION>/`
//! (set via the `GENCUBE_PRUNING_CACHE` env var that `cube-core` already reads).
//! The version suffix comes from `cube_core::pruning::CACHE_VERSION` so cube-core
//! schema bumps invalidate the Tauri-side cache automatically.

mod messages;

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_dialog::DialogExt;

use cube_core::pruning::{get_pruning_tables_with_progress, BuildProgressEvent, BuildSource, CACHE_VERSION};
use cube_core::search::{
    build_search_input, run_search, SearchCallbacks, SearchProgress, SearchSolution,
};

use messages::{ProjectOpenResult, SearchRequest, WorkerEvent};

const WORKER_EVENT: &str = "worker-event";

/// Snapshot of the pruning-table lifecycle. Updated by the background build
/// thread as events fire, queried by the renderer on mount so events that fly
/// before `listen()` is registered aren't lost.
#[derive(Default, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PruningStatus {
    ready: bool,
    last_event: Option<WorkerEvent>,
}

#[derive(Default)]
struct PruningState(Mutex<PruningStatus>);

/// In-flight jobs. Each `start_search` registers an `AtomicBool` that the search
/// thread polls via the `should_cancel` callback. `cancel_search` looks up the
/// flag by `jobId` and sets it.
#[derive(Default)]
struct JobRegistry {
    flags: Mutex<HashMap<String, Arc<AtomicBool>>>,
}

impl JobRegistry {
    fn register(&self, job_id: &str) -> Arc<AtomicBool> {
        let flag = Arc::new(AtomicBool::new(false));
        self.flags
            .lock()
            .expect("JobRegistry poisoned")
            .insert(job_id.to_string(), Arc::clone(&flag));
        flag
    }

    fn cancel(&self, job_id: &str) {
        if let Some(flag) = self.flags.lock().expect("JobRegistry poisoned").get(job_id) {
            flag.store(true, Ordering::Relaxed);
        }
    }

    fn forget(&self, job_id: &str) {
        self.flags
            .lock()
            .expect("JobRegistry poisoned")
            .remove(job_id);
    }
}

fn emit(app: &AppHandle, ev: WorkerEvent) {
    if let Err(e) = app.emit(WORKER_EVENT, ev) {
        eprintln!("emit {} failed: {}", WORKER_EVENT, e);
    }
}

fn map_build_event(e: BuildProgressEvent) -> WorkerEvent {
    match e {
        BuildProgressEvent::StepStart {
            step,
            total,
            name,
            source,
        } => WorkerEvent::TableBuildStart {
            step,
            total,
            name,
            source: match source {
                BuildSource::Cache => "cache",
                BuildSource::Build => "build",
            },
        },
        BuildProgressEvent::StepDone {
            step,
            total,
            name,
            elapsed_ms,
        } => WorkerEvent::TableBuildStepDone {
            step,
            total,
            name,
            elapsed_ms,
        },
        BuildProgressEvent::AllDone { elapsed_ms } => {
            WorkerEvent::TableBuildAllDone { elapsed_ms }
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct EmptyAck {}

#[tauri::command]
fn start_search(
    app: AppHandle,
    registry: State<'_, Arc<JobRegistry>>,
    req: SearchRequest,
) -> Result<EmptyAck, String> {
    let job_id = req.job_id.clone();
    let cancel_flag = registry.register(&job_id);
    let registry_arc = (*registry).clone();
    let app_clone = app.clone();

    thread::spawn(move || {
        let job_id = req.job_id.clone();
        let result = (|| -> Result<(), String> {
            let input = build_search_input(&req)?;

            let app_sol = app_clone.clone();
            let job_sol = job_id.clone();
            let app_prog = app_clone.clone();
            let job_prog = job_id.clone();
            let cancel_for_cb = Arc::clone(&cancel_flag);

            let cb = SearchCallbacks {
                on_solution: Some(Box::new(move |sol: &SearchSolution| {
                    emit(
                        &app_sol,
                        WorkerEvent::Solution {
                            job_id: job_sol.clone(),
                            moves: sol.moves.clone(),
                            length: sol.length,
                        },
                    );
                })),
                on_progress: Some(Box::new(move |p: &SearchProgress| {
                    emit(
                        &app_prog,
                        WorkerEvent::Progress {
                            job_id: job_prog.clone(),
                            depth: p.depth,
                            solutions_found: p.solutions_found,
                            nodes_explored: p.nodes_explored,
                            elapsed_ms: p.elapsed_ms,
                        },
                    );
                })),
                should_cancel: Some(Box::new(move || cancel_for_cb.load(Ordering::Relaxed))),
            };

            // Phase v4-9: try cubie dispatch first. The cubie engine handles
            // masked preset cases (OLL/F2L/OCLL/EOLL etc.) ~30-100x faster
            // than the legacy facelet engine because of partial-coord PDBs
            // and a permutation-only state. Returns None when the request
            // shape (e.g. full-mask PLL, custom mask, wide moves) doesn't
            // fit; in that case fall through to legacy.
            let cubie_stats = {
                let app_cubie = app_clone.clone();
                let job_cubie = job_id.clone();
                let cancel_cubie = Arc::clone(&cancel_flag);
                let mut emit_cubie = move |length: usize, moves: &[usize]| {
                    emit(
                        &app_cubie,
                        WorkerEvent::Solution {
                            job_id: job_cubie.clone(),
                            moves: moves.to_vec(),
                            length,
                        },
                    );
                };
                let cancel_fn = move || cancel_cubie.load(Ordering::Relaxed);
                let cancel_fn_ref: &(dyn Fn() -> bool + Sync) = &cancel_fn;
                cube_core::cubie_search::try_cubie_dispatch(
                    &req,
                    Some(&mut emit_cubie),
                    cancel_fn_ref,
                )
            };

            let stats = match cubie_stats {
                Some(s) => s,
                None => run_search(&input, cb),
            };
            emit(
                &app_clone,
                WorkerEvent::Complete {
                    job_id: job_id.clone(),
                    total_solutions: stats.total_solutions,
                    time_ms: stats.time_ms,
                    nodes_explored: stats.nodes_explored,
                    shortest_length: stats.shortest_length,
                },
            );
            Ok(())
        })();

        if let Err(msg) = result {
            emit(
                &app_clone,
                WorkerEvent::Error {
                    job_id: job_id.clone(),
                    message: msg,
                },
            );
        }
        registry_arc.forget(&job_id);
    });

    Ok(EmptyAck {})
}

#[tauri::command]
fn cancel_search(
    app: AppHandle,
    registry: State<'_, Arc<JobRegistry>>,
    job_id: String,
) -> Result<EmptyAck, String> {
    registry.cancel(&job_id);
    // Match Electron behavior: emit a synthetic `complete` so the UI clears
    // the "running" indicator without waiting for the search thread to notice
    // the cancellation flag.
    emit(
        &app,
        WorkerEvent::Complete {
            job_id,
            total_solutions: 0,
            time_ms: 0,
            nodes_explored: 0,
            shortest_length: None,
        },
    );
    Ok(EmptyAck {})
}

#[tauri::command]
fn save_project(
    app: AppHandle,
    default_name: String,
    json: String,
) -> Result<Option<String>, String> {
    let path_opt = app
        .dialog()
        .file()
        .add_filter("gen-search project", &["gencube.json", "json"])
        .set_file_name(&default_name)
        .blocking_save_file();
    let Some(path) = path_opt else {
        return Ok(None);
    };
    let p = path
        .as_path()
        .ok_or_else(|| "selected path is not a regular filesystem path".to_string())?
        .to_path_buf();
    std::fs::write(&p, json).map_err(|e| format!("write {}: {}", p.display(), e))?;
    Ok(Some(p.display().to_string()))
}

#[tauri::command]
fn get_pruning_status(state: State<'_, Arc<PruningState>>) -> PruningStatus {
    state.0.lock().expect("PruningState poisoned").clone()
}

#[tauri::command]
fn save_solutions_csv(
    app: AppHandle,
    default_name: String,
    csv: String,
) -> Result<Option<String>, String> {
    let path_opt = app
        .dialog()
        .file()
        .add_filter("CSV", &["csv"])
        .set_file_name(&default_name)
        .blocking_save_file();
    let Some(path) = path_opt else {
        return Ok(None);
    };
    let p = path
        .as_path()
        .ok_or_else(|| "selected path is not a regular filesystem path".to_string())?
        .to_path_buf();
    std::fs::write(&p, csv).map_err(|e| format!("write {}: {}", p.display(), e))?;
    Ok(Some(p.display().to_string()))
}

fn settings_path(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("app_data_dir: {}", e))?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("mkdir {}: {}", dir.display(), e))?;
    Ok(dir.join("settings.json"))
}

#[tauri::command]
fn load_settings(app: AppHandle) -> Result<Option<String>, String> {
    let path = settings_path(&app)?;
    match std::fs::read_to_string(&path) {
        Ok(s) => Ok(Some(s)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("read {}: {}", path.display(), e)),
    }
}

#[tauri::command]
fn save_settings(app: AppHandle, json: String) -> Result<(), String> {
    let path = settings_path(&app)?;
    std::fs::write(&path, json).map_err(|e| format!("write {}: {}", path.display(), e))
}

#[tauri::command]
fn open_project(app: AppHandle) -> Result<Option<ProjectOpenResult>, String> {
    let path_opt = app
        .dialog()
        .file()
        .add_filter("gen-search project", &["gencube.json", "json"])
        .blocking_pick_file();
    let Some(path) = path_opt else {
        return Ok(None);
    };
    let p = path
        .as_path()
        .ok_or_else(|| "selected path is not a regular filesystem path".to_string())?
        .to_path_buf();
    let data = std::fs::read_to_string(&p).map_err(|e| format!("read {}: {}", p.display(), e))?;
    Ok(Some(ProjectOpenResult {
        path: p.display().to_string(),
        data,
    }))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let registry = Arc::new(JobRegistry::default());
    let pruning_state = Arc::new(PruningState::default());

    let pruning_for_setup = Arc::clone(&pruning_state);

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .manage(registry)
        .manage(pruning_state)
        .setup(move |app| {
            // Pin the pruning-table disk cache to the app's data dir so the
            // 88 MB Corner PDB only builds once per install.
            if let Ok(dir) = app.path().app_data_dir() {
                let cache = dir.join(format!("pruning-cache-{}", CACHE_VERSION));
                std::env::set_var("GENCUBE_PRUNING_CACHE", cache);
            }
            // Eagerly load the pruning tables on startup. Two consumers:
            //  (a) live `worker-event` emits → updates UI banner in real time
            //  (b) `PruningState` snapshot → renderer queries it on mount in
            //      case the live events fired before `listen()` was registered
            //      (cache-hit path completes in ~110 ms; the React app may
            //      not have mounted yet, so without the snapshot the banner
            //      sticks on the initial "loading…" text forever).
            let handle = app.handle().clone();
            let state = Arc::clone(&pruning_for_setup);
            thread::spawn(move || {
                get_pruning_tables_with_progress(|e| {
                    let ev = map_build_event(e);
                    {
                        let mut s = state.0.lock().expect("PruningState poisoned");
                        if matches!(ev, WorkerEvent::TableBuildAllDone { .. }) {
                            s.ready = true;
                        }
                        s.last_event = Some(ev.clone());
                    }
                    emit(&handle, ev);
                });
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            start_search,
            cancel_search,
            save_project,
            open_project,
            get_pruning_status,
            save_solutions_csv,
            load_settings,
            save_settings,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
