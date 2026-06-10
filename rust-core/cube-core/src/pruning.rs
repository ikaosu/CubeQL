//! Kociemba-style pruning tables.
//!
//! Four BFS distance tables — see `pruning.ts` for the math. Disk cache layout
//! is byte-for-byte compatible with the TS implementation (so existing
//! `gen-search-pruning-v3/*.bin` files load directly).
//!
//! Version history:
//!   v1 — 45-move BFS (face/wide/slice only). x/y/z rotations were excluded
//!         from the search engine's pruning lookup via a `tainted_stack`
//!         fallback (mismatch heuristic only), making rotation-using DSL
//!         patterns ~100-1000x slower.
//!   v2 — 54-move BFS (rotations included). Rotations now have admissible
//!         coord transitions; the engine drops the tainted fallback and
//!         keeps full pruning across the entire path.
//!   v3 — Multi-source BFS: the 24 rotated-solved coord values are all
//!         distance-0 sources. Heuristic returns 0 for any physically-solved
//!         state regardless of orientation (rotation-tolerant goal).

use std::env;
use std::fs;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

use crate::coord::{
    encode_corner_ori, encode_corner_perm_from_state, encode_edge_ori, encode_slice,
    get_move_tables, NUM_CO_COORDS, NUM_CP_COORDS, NUM_EO_COORDS, NUM_SLICE_COORDS,
};
use crate::moves::MOVE_COUNT;
use crate::rotations::rotated_solved_facelets;

pub const CACHE_VERSION: &str = "v3";
const UNREACHED: u8 = 0xff;

fn cache_dir() -> PathBuf {
    if let Ok(p) = env::var("GENCUBE_PRUNING_CACHE") {
        return PathBuf::from(p);
    }
    let mut d = env::temp_dir();
    d.push(format!("gen-search-pruning-{}", CACHE_VERSION));
    d
}

fn load_from_cache(name: &str, expected_size: usize) -> Option<Vec<u8>> {
    let path = cache_dir().join(format!("{}.bin", name));
    let bytes = fs::read(&path).ok()?;
    if bytes.len() == expected_size {
        Some(bytes)
    } else {
        None
    }
}

fn save_to_cache(name: &str, data: &[u8]) {
    let dir = cache_dir();
    if fs::create_dir_all(&dir).is_err() {
        return;
    }
    let _ = fs::write(dir.join(format!("{}.bin", name)), data);
}

#[derive(Debug, Clone)]
pub enum BuildProgressEvent {
    StepStart {
        step: usize,
        total: usize,
        name: String,
        source: BuildSource,
    },
    StepDone {
        step: usize,
        total: usize,
        name: String,
        elapsed_ms: u128,
    },
    AllDone {
        elapsed_ms: u128,
    },
}

#[derive(Debug, Clone, Copy)]
pub enum BuildSource {
    Cache,
    Build,
}

pub struct PruningTables {
    pub co_slice: Vec<u8>,
    pub eo_slice: Vec<u8>,
    pub co_eo: Vec<u8>,
    pub cp_co: Vec<u8>,
}

static PRUNING_TABLES: OnceLock<PruningTables> = OnceLock::new();
static BUILDING: Mutex<bool> = Mutex::new(false);

/// Compute the 24 source coord pairs `(axis_value, slice_value)` for the
/// rotated-solved goal states. Each source maps the rotated-solved facelet
/// through the canonical coord encoders for the given axis encoder + slice.
fn rotated_solved_sources<F>(encode_axis: F) -> Vec<(u32, u32)>
where
    F: Fn(&[i8; crate::facelet::FACELET_COUNT]) -> u32,
{
    let mut out = Vec::with_capacity(24);
    for state in rotated_solved_facelets() {
        let axis = encode_axis(state);
        let slice = encode_slice(state) as u32;
        out.push((axis, slice));
    }
    out.sort_unstable();
    out.dedup();
    out
}

fn bfs_joint_table_multi(
    num_axis: usize,
    axis_table: &[u32],
    slice_table: &[u32],
    sources: &[(u32, u32)],
) -> Vec<u8> {
    let total = num_axis * NUM_SLICE_COORDS;
    let mut t = vec![UNREACHED; total];
    let mut frontier: Vec<u32> = Vec::with_capacity(sources.len());
    for &(axis, slice) in sources {
        let origin = (axis as usize) * NUM_SLICE_COORDS + slice as usize;
        if t[origin] == UNREACHED {
            t[origin] = 0;
            frontier.push(origin as u32);
        }
    }
    let mut depth: u8 = 0;
    while !frontier.is_empty() {
        let next_depth = depth + 1;
        let mut next: Vec<u32> = Vec::with_capacity(frontier.len());
        for &c in &frontier {
            let axis = (c as usize) / NUM_SLICE_COORDS;
            let slice = (c as usize) % NUM_SLICE_COORDS;
            for m in 0..MOVE_COUNT {
                let new_axis = axis_table[m * num_axis + axis] as usize;
                let new_slice = slice_table[m * NUM_SLICE_COORDS + slice] as usize;
                let new_coord = new_axis * NUM_SLICE_COORDS + new_slice;
                if t[new_coord] == UNREACHED {
                    t[new_coord] = next_depth;
                    next.push(new_coord as u32);
                }
            }
        }
        frontier = next;
        depth = next_depth;
        assert!(depth <= 30, "BFS exceeded safety bound");
    }
    t
}

fn bfs_pair_table_multi(
    num_a: usize,
    num_b: usize,
    a_table: &[u32],
    b_table: &[u32],
    sources: &[(u32, u32)],
) -> Vec<u8> {
    let total = num_a * num_b;
    let mut t = vec![UNREACHED; total];
    let mut frontier: Vec<u32> = Vec::with_capacity(sources.len());
    for &(a, b) in sources {
        let origin = (a as usize) * num_b + b as usize;
        if t[origin] == UNREACHED {
            t[origin] = 0;
            frontier.push(origin as u32);
        }
    }
    let mut depth: u8 = 0;
    while !frontier.is_empty() {
        let next_depth = depth + 1;
        let mut next: Vec<u32> = Vec::with_capacity(frontier.len());
        for &c in &frontier {
            let av = (c as usize) / num_b;
            let bv = (c as usize) % num_b;
            for m in 0..MOVE_COUNT {
                let new_a = a_table[m * num_a + av] as usize;
                let new_b = b_table[m * num_b + bv] as usize;
                let new_coord = new_a * num_b + new_b;
                if t[new_coord] == UNREACHED {
                    t[new_coord] = next_depth;
                    next.push(new_coord as u32);
                }
            }
        }
        frontier = next;
        depth = next_depth;
        assert!(depth <= 30, "BFS exceeded safety bound");
    }
    t
}

pub fn get_pruning_tables_with_progress<F: FnMut(BuildProgressEvent)>(
    mut on_progress: F,
) -> &'static PruningTables {
    if let Some(t) = PRUNING_TABLES.get() {
        return t;
    }
    let _guard = BUILDING
        .lock()
        .expect("pruning table BUILDING mutex poisoned");
    // Double-check after acquiring the lock.
    if let Some(t) = PRUNING_TABLES.get() {
        return t;
    }

    let overall_start = Instant::now();
    let move_tables = get_move_tables();

    let specs: Vec<(&str, usize, Box<dyn Fn() -> Vec<u8>>)> = vec![
        (
            "co_slice",
            NUM_CO_COORDS * NUM_SLICE_COORDS,
            Box::new(|| {
                let mt = get_move_tables();
                let sources = rotated_solved_sources(|s| encode_corner_ori(s) as u32);
                bfs_joint_table_multi(NUM_CO_COORDS, &mt.co, &mt.slice, &sources)
            }),
        ),
        (
            "eo_slice",
            NUM_EO_COORDS * NUM_SLICE_COORDS,
            Box::new(|| {
                let mt = get_move_tables();
                let sources = rotated_solved_sources(|s| encode_edge_ori(s) as u32);
                bfs_joint_table_multi(NUM_EO_COORDS, &mt.eo, &mt.slice, &sources)
            }),
        ),
        (
            "co_eo",
            NUM_CO_COORDS * NUM_EO_COORDS,
            Box::new(|| {
                let mt = get_move_tables();
                let mut sources: Vec<(u32, u32)> = rotated_solved_facelets()
                    .iter()
                    .map(|s| (encode_corner_ori(s) as u32, encode_edge_ori(s) as u32))
                    .collect();
                sources.sort_unstable();
                sources.dedup();
                bfs_pair_table_multi(NUM_CO_COORDS, NUM_EO_COORDS, &mt.co, &mt.eo, &sources)
            }),
        ),
        (
            "cp_co",
            NUM_CP_COORDS * NUM_CO_COORDS,
            Box::new(|| {
                let mt = get_move_tables();
                let mut sources: Vec<(u32, u32)> = rotated_solved_facelets()
                    .iter()
                    .filter_map(|s| {
                        let cp = encode_corner_perm_from_state(s);
                        if cp < 0 {
                            None
                        } else {
                            Some((cp as u32, encode_corner_ori(s) as u32))
                        }
                    })
                    .collect();
                sources.sort_unstable();
                sources.dedup();
                bfs_pair_table_multi(NUM_CP_COORDS, NUM_CO_COORDS, &mt.cp, &mt.co, &sources)
            }),
        ),
    ];

    let total = specs.len();
    let mut produced: Vec<Vec<u8>> = Vec::with_capacity(total);
    for (idx, (name, expected_size, build)) in specs.into_iter().enumerate() {
        let cached = load_from_cache(name, expected_size);
        let source = if cached.is_some() {
            BuildSource::Cache
        } else {
            BuildSource::Build
        };
        on_progress(BuildProgressEvent::StepStart {
            step: idx + 1,
            total,
            name: name.to_string(),
            source,
        });
        let start = Instant::now();
        let data = match cached {
            Some(d) => d,
            None => {
                let d = build();
                save_to_cache(name, &d);
                d
            }
        };
        on_progress(BuildProgressEvent::StepDone {
            step: idx + 1,
            total,
            name: name.to_string(),
            elapsed_ms: start.elapsed().as_millis(),
        });
        produced.push(data);
    }
    on_progress(BuildProgressEvent::AllDone {
        elapsed_ms: overall_start.elapsed().as_millis(),
    });

    let _ = move_tables; // ensure built
    let tables = PruningTables {
        co_slice: produced.remove(0),
        eo_slice: produced.remove(0),
        co_eo: produced.remove(0),
        cp_co: produced.remove(0),
    };
    PRUNING_TABLES
        .set(tables)
        .ok()
        .expect("pruning tables initialised twice");
    PRUNING_TABLES.get().unwrap()
}

pub fn get_pruning_tables() -> &'static PruningTables {
    get_pruning_tables_with_progress(|_| {})
}
