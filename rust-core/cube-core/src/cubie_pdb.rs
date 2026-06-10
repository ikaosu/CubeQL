//! Cubie-coord pruning database (PDB) builders.
//!
//! For now this exposes the **corner orientation** PDB only — 2187 entries
//! storing the minimum number of face-turn moves needed to reach each
//! `corner_ori_coord` value from the solved state's CO coord (0).
//!
//! BFS is performed in coord space using a pre-built move table
//! (`co_move_table`). Result is admissible: PDB[c] ≤ true HTM distance from
//! state with CO=c to solved.
//!
//! Larger PDBs (EO, UD-slice, CO×EO joint, CP×CO) follow the same recipe
//! and are TODO for Phase v4-6.

use std::fs;
use std::path::PathBuf;
use std::sync::OnceLock;

use crate::cubie::{apply_cubie_move, build_face_moves, CornerCubie, CubieState, EdgeCubie, NUM_CORNERS, NUM_EDGES};
use crate::cubie_coord::{
    corner_ori_coord, corner_perm_coord, decode_corner_ori_coord, edge_ori_coord,
    decode_edge_ori_coord, slice_coord, NUM_CO_COORDS, NUM_CP_COORDS, NUM_EO_COORDS,
    NUM_SLICE_COORDS,
};

const CO_CACHE_FILE: &str = "co_partial.prun";
const EO_CACHE_FILE: &str = "eo_partial.prun";
const CP_CACHE_FILE: &str = "cp_partial.prun";
const CO_EO_CACHE_FILE: &str = "co_eo_joint.prun";
const CO_SLICE_CACHE_FILE: &str = "co_slice_joint.prun";
const EO_SLICE_CACHE_FILE: &str = "eo_slice_joint.prun";
pub const CACHE_VERSION_V4: &str = "v4";

/// Size of the joint CO×EO PDB (2187 × 2048 ≈ 4.27 MB).
pub const NUM_CO_EO_COORDS: usize = NUM_CO_COORDS * NUM_EO_COORDS;
/// Size of the joint CO×slice PDB (2187 × 495 ≈ 1.03 MB).
pub const NUM_CO_SLICE_COORDS: usize = NUM_CO_COORDS * crate::cubie_coord::NUM_SLICE_COORDS;
/// Size of the joint EO×slice PDB (2048 × 495 ≈ 0.97 MB).
pub const NUM_EO_SLICE_COORDS: usize = NUM_EO_COORDS * crate::cubie_coord::NUM_SLICE_COORDS;

fn cache_dir() -> PathBuf {
    if let Ok(p) = std::env::var("GENCUBE_PRUNING_CACHE_V4") {
        return PathBuf::from(p);
    }
    let mut d = std::env::temp_dir();
    d.push(format!("gen-search-pruning-{}", CACHE_VERSION_V4));
    d
}

/// Return the precomputed CO move table. Lazily built and shared with PDB
/// builders that need a CO-coord transition (joint CO×EO etc.).
pub fn co_move_table() -> &'static Vec<[u32; 18]> {
    static CO_MOVE_TABLE: OnceLock<Vec<[u32; 18]>> = OnceLock::new();
    CO_MOVE_TABLE.get_or_init(build_co_move_table)
}

/// Return the precomputed EO move table.
pub fn eo_move_table() -> &'static Vec<[u32; 18]> {
    static EO_MOVE_TABLE: OnceLock<Vec<[u32; 18]>> = OnceLock::new();
    EO_MOVE_TABLE.get_or_init(build_eo_move_table)
}

/// Return the precomputed slice move table.
pub fn slice_move_table() -> &'static Vec<[u32; 18]> {
    static SLICE_MOVE_TABLE: OnceLock<Vec<[u32; 18]>> = OnceLock::new();
    SLICE_MOVE_TABLE.get_or_init(build_slice_move_table)
}

/// Return the precomputed CP move table (40320 × 18). Lazily built; first
/// call takes ~150ms on a modern CPU (decoding 40320 Lehmer permutations).
pub fn cp_move_table() -> &'static Vec<[u32; 18]> {
    static CP_MOVE_TABLE: OnceLock<Vec<[u32; 18]>> = OnceLock::new();
    CP_MOVE_TABLE.get_or_init(build_cp_move_table)
}

/// Apply all 18 face moves to each CO coord. Result: `table[co_coord][move_idx] = new_co_coord`.
fn build_co_move_table() -> Vec<[u32; 18]> {
    let face_moves = build_face_moves();
    let mut table = Vec::with_capacity(NUM_CO_COORDS);
    for co_index in 0..NUM_CO_COORDS {
        let oris = decode_corner_ori_coord(co_index as u32);
        let mut state = CubieState::solved();
        for i in 0..NUM_CORNERS {
            state.corners[i].ori = oris[i];
        }
        let mut row = [0u32; 18];
        for m in 0..18 {
            let new_state = apply_cubie_move(&state, &face_moves[m]);
            let (new_co, _) = corner_ori_coord(&new_state);
            row[m] = new_co;
        }
        table.push(row);
    }
    table
}

/// BFS over CO coord space. Returns 2187-byte table: `table[c]` = minimum
/// number of face turns to reach solved (CO=0) from state with CO=c.
fn bfs_co_pdb() -> Vec<u8> {
    let move_table = co_move_table();
    const UNREACHED: u8 = 0xff;
    let mut depths = vec![UNREACHED; NUM_CO_COORDS];
    depths[0] = 0;
    let mut current: Vec<u32> = vec![0];
    let mut depth = 0u8;
    while !current.is_empty() {
        let mut next: Vec<u32> = Vec::new();
        for &coord in &current {
            for m in 0..18 {
                let next_coord = move_table[coord as usize][m];
                if depths[next_coord as usize] == UNREACHED {
                    depths[next_coord as usize] = depth + 1;
                    next.push(next_coord);
                }
            }
        }
        depth += 1;
        current = next;
    }
    debug_assert!(depths.iter().all(|&d| d != UNREACHED));
    depths
}

/// Return the corner-orientation PDB. Loads from disk cache if present,
/// otherwise builds via BFS and saves the cache.
pub fn co_pdb() -> &'static [u8] {
    static CO_PDB: OnceLock<Vec<u8>> = OnceLock::new();
    CO_PDB.get_or_init(|| {
        let path = cache_dir().join(CO_CACHE_FILE);
        if let Ok(bytes) = fs::read(&path) {
            if bytes.len() == NUM_CO_COORDS {
                return bytes;
            }
        }
        let depths = bfs_co_pdb();
        if fs::create_dir_all(cache_dir()).is_ok() {
            let _ = fs::write(&path, &depths);
        }
        depths
    })
}

// ----- Edge orientation PDB -----------------------------------------------

fn build_eo_move_table() -> Vec<[u32; 18]> {
    let face_moves = build_face_moves();
    let mut table = Vec::with_capacity(NUM_EO_COORDS);
    for eo_index in 0..NUM_EO_COORDS {
        let oris = decode_edge_ori_coord(eo_index as u32);
        let mut state = CubieState::solved();
        for i in 0..NUM_EDGES {
            state.edges[i].ori = oris[i];
        }
        let mut row = [0u32; 18];
        for m in 0..18 {
            let new_state = apply_cubie_move(&state, &face_moves[m]);
            let (new_eo, _) = edge_ori_coord(&new_state);
            row[m] = new_eo;
        }
        table.push(row);
    }
    table
}

fn bfs_eo_pdb() -> Vec<u8> {
    let move_table = eo_move_table();
    const UNREACHED: u8 = 0xff;
    let mut depths = vec![UNREACHED; NUM_EO_COORDS];
    depths[0] = 0;
    let mut current: Vec<u32> = vec![0];
    let mut depth = 0u8;
    while !current.is_empty() {
        let mut next: Vec<u32> = Vec::new();
        for &coord in &current {
            for m in 0..18 {
                let next_coord = move_table[coord as usize][m];
                if depths[next_coord as usize] == UNREACHED {
                    depths[next_coord as usize] = depth + 1;
                    next.push(next_coord);
                }
            }
        }
        depth += 1;
        current = next;
    }
    depths
}

pub fn eo_pdb() -> &'static [u8] {
    static EO_PDB: OnceLock<Vec<u8>> = OnceLock::new();
    EO_PDB.get_or_init(|| {
        let path = cache_dir().join(EO_CACHE_FILE);
        if let Ok(bytes) = fs::read(&path) {
            if bytes.len() == NUM_EO_COORDS {
                return bytes;
            }
        }
        let depths = bfs_eo_pdb();
        if fs::create_dir_all(cache_dir()).is_ok() {
            let _ = fs::write(&path, &depths);
        }
        depths
    })
}

// ----- Corner permutation PDB ---------------------------------------------
//
// 40320 entries. Indexed by Lehmer code of the 8-corner permutation.
// BFS in CP coord space. Used heavily for PLL-type queries where corners
// are already oriented but permuted.

/// Decode a Lehmer-coded corner permutation back into a `[u8; 8]` array.
fn decode_corner_perm(coord: u32) -> [u8; NUM_CORNERS] {
    let mut perm = [0u8; NUM_CORNERS];
    let mut available: Vec<u8> = (0..NUM_CORNERS as u8).collect();
    let mut c = coord;
    // We need to invert the encoding loop. The encode loop is:
    //   coord = 0
    //   for i in 0..N: coord = coord * (N-i) + count_smaller_after(perm[i])
    // To invert: walk i from N-1 down to 0, pulling out count_smaller at each step.
    // Equivalently, since the multiplier sequence is N, N-1, ..., 1, factor from the back.
    let mut counts = [0u32; NUM_CORNERS];
    for i in (0..NUM_CORNERS).rev() {
        let factor = (NUM_CORNERS - i) as u32;
        counts[i] = c % factor;
        c /= factor;
    }
    for i in 0..NUM_CORNERS {
        let idx = counts[i] as usize;
        // pick (counts[i])-th smallest among available - skip indices that
        // are larger than the i-th piece's value. Standard Lehmer decode:
        // available is sorted ascending; counts[i] = number of pieces less
        // than perm[i] still remaining. perm[i] = (idx+1)-th smallest? No
        // — count_smaller_after counts how many in REMAINING positions are
        // smaller. With our convention, perm[i] is the (idx)-th value in
        // available sorted descending... let me just match the encoding.
        //
        // Actually simpler: count_smaller = how many remaining values are
        // LESS than perm[i]. So if perm[i] is the j-th smallest in
        // available, count_smaller = j (0-indexed). Therefore
        // perm[i] = available[counts[i]].
        let val = available[idx];
        perm[i] = val;
        available.remove(idx);
    }
    perm
}

fn build_cp_move_table() -> Vec<[u32; 18]> {
    let face_moves = build_face_moves();
    let mut table = vec![[0u32; 18]; NUM_CP_COORDS];
    for cp_index in 0..NUM_CP_COORDS {
        let perm = decode_corner_perm(cp_index as u32);
        let mut state = CubieState::solved();
        for slot in 0..NUM_CORNERS {
            state.corners[slot] = CornerCubie {
                pos: perm[slot],
                ori: 0,
            };
        }
        for m in 0..18 {
            let new_state = apply_cubie_move(&state, &face_moves[m]);
            let (new_cp, _) = corner_perm_coord(&new_state);
            table[cp_index][m] = new_cp;
        }
    }
    table
}

fn bfs_cp_pdb() -> Vec<u8> {
    let move_table = cp_move_table();
    const UNREACHED: u8 = 0xff;
    let mut depths = vec![UNREACHED; NUM_CP_COORDS];
    depths[0] = 0;
    let mut current: Vec<u32> = vec![0];
    let mut depth = 0u8;
    while !current.is_empty() {
        let mut next: Vec<u32> = Vec::new();
        for &coord in &current {
            for m in 0..18 {
                let next_coord = move_table[coord as usize][m];
                if depths[next_coord as usize] == UNREACHED {
                    depths[next_coord as usize] = depth + 1;
                    next.push(next_coord);
                }
            }
        }
        depth += 1;
        current = next;
    }
    depths
}

pub fn cp_pdb() -> &'static [u8] {
    static CP_PDB: OnceLock<Vec<u8>> = OnceLock::new();
    CP_PDB.get_or_init(|| {
        let path = cache_dir().join(CP_CACHE_FILE);
        if let Ok(bytes) = fs::read(&path) {
            if bytes.len() == NUM_CP_COORDS {
                return bytes;
            }
        }
        let depths = bfs_cp_pdb();
        if fs::create_dir_all(cache_dir()).is_ok() {
            let _ = fs::write(&path, &depths);
        }
        depths
    })
}

// ----- UD-slice membership PDB --------------------------------------------
//
// 495 entries. Indexed by the colex combination of E-slice edge positions.
// Solved state has slice coord = 494 (all 4 slice edges in slots 8..12).

fn binomial_u32(n: u32, k: u32) -> u32 {
    if k > n {
        return 0;
    }
    let mut r = 1u64;
    let k = k.min(n - k);
    for i in 0..k {
        r = r * (n as u64 - i as u64) / (i as u64 + 1);
    }
    r as u32
}

fn decode_slice_coord(coord: u32) -> [bool; NUM_EDGES] {
    let mut result = [false; NUM_EDGES];
    let mut c = coord;
    let mut k = 4u32;
    for i in (0..NUM_EDGES as u32).rev() {
        if k == 0 {
            break;
        }
        let bn = binomial_u32(i, k);
        if c >= bn {
            result[i as usize] = true;
            c -= bn;
            k -= 1;
        }
    }
    result
}

fn build_slice_move_table() -> Vec<[u32; 18]> {
    let face_moves = build_face_moves();
    let mut table = vec![[0u32; 18]; NUM_SLICE_COORDS];
    for slice_index in 0..NUM_SLICE_COORDS {
        let membership = decode_slice_coord(slice_index as u32);
        let mut state = CubieState::solved();
        // Reconstruct a state with this slice membership. Slots marked true
        // get slice-edge pieces (8..12), others get UD-edge pieces (0..8).
        // EO/CO don't affect slice transition.
        let mut next_slice = 8u8;
        let mut next_ud = 0u8;
        for slot in 0..NUM_EDGES {
            if membership[slot] {
                state.edges[slot] = EdgeCubie {
                    pos: next_slice,
                    ori: 0,
                };
                next_slice += 1;
            } else {
                state.edges[slot] = EdgeCubie {
                    pos: next_ud,
                    ori: 0,
                };
                next_ud += 1;
            }
        }
        for m in 0..18 {
            let new_state = apply_cubie_move(&state, &face_moves[m]);
            let (new_slice, _) = slice_coord(&new_state);
            table[slice_index][m] = new_slice;
        }
    }
    table
}

// ----- Joint CO × EO PDB --------------------------------------------------
//
// 2187 × 2048 = 4_478_976 entries ≈ 4.27 MB. Because face-turn transitions
// on CO and EO are independent (each is a function of its own coord plus
// the move), the joint BFS reuses `co_move_table()` and `eo_move_table()`.
//
// Index: `pdb[co * NUM_EO_COORDS + eo]`. Solved index = 0.

fn bfs_co_eo_pdb() -> Vec<u8> {
    let co_table = co_move_table();
    let eo_table = eo_move_table();
    const UNREACHED: u8 = 0xff;
    let n = NUM_CO_EO_COORDS;
    let mut depths = vec![UNREACHED; n];
    depths[0] = 0; // co=0, eo=0
    let mut current: Vec<u32> = vec![0];
    let mut depth = 0u8;
    while !current.is_empty() {
        let mut next: Vec<u32> = Vec::new();
        for &idx in &current {
            let co = (idx as usize) / NUM_EO_COORDS;
            let eo = (idx as usize) % NUM_EO_COORDS;
            let co_row = &co_table[co];
            let eo_row = &eo_table[eo];
            for m in 0..18 {
                let new_co = co_row[m] as usize;
                let new_eo = eo_row[m] as usize;
                let new_idx = new_co * NUM_EO_COORDS + new_eo;
                if depths[new_idx] == UNREACHED {
                    depths[new_idx] = depth + 1;
                    next.push(new_idx as u32);
                }
            }
        }
        depth += 1;
        current = next;
    }
    depths
}

pub fn co_eo_pdb() -> &'static [u8] {
    static CO_EO_PDB: OnceLock<Vec<u8>> = OnceLock::new();
    CO_EO_PDB.get_or_init(|| {
        let path = cache_dir().join(CO_EO_CACHE_FILE);
        if let Ok(bytes) = fs::read(&path) {
            if bytes.len() == NUM_CO_EO_COORDS {
                return bytes;
            }
        }
        let depths = bfs_co_eo_pdb();
        if fs::create_dir_all(cache_dir()).is_ok() {
            let _ = fs::write(&path, &depths);
        }
        depths
    })
}

// ----- Joint CO × slice PDB -----------------------------------------------
//
// 2187 × 495 ≈ 1.03 MB. Useful when EO is unknown (e.g. OCLL mask) and we
// still want a tighter LB than CO alone. Solved index = 0*495 + 494 = 494.

const SOLVED_SLICE_COORD: u32 = 494;

fn bfs_co_slice_pdb() -> Vec<u8> {
    let co_table = co_move_table();
    let slice_table = slice_move_table();
    const UNREACHED: u8 = 0xff;
    let n = NUM_CO_SLICE_COORDS;
    let mut depths = vec![UNREACHED; n];
    let start = 0 * NUM_SLICE_COORDS + SOLVED_SLICE_COORD as usize;
    depths[start] = 0;
    let mut current: Vec<u32> = vec![start as u32];
    let mut depth = 0u8;
    while !current.is_empty() {
        let mut next: Vec<u32> = Vec::new();
        for &idx in &current {
            let co = (idx as usize) / NUM_SLICE_COORDS;
            let sl = (idx as usize) % NUM_SLICE_COORDS;
            let co_row = &co_table[co];
            let sl_row = &slice_table[sl];
            for m in 0..18 {
                let new_co = co_row[m] as usize;
                let new_sl = sl_row[m] as usize;
                let new_idx = new_co * NUM_SLICE_COORDS + new_sl;
                if depths[new_idx] == UNREACHED {
                    depths[new_idx] = depth + 1;
                    next.push(new_idx as u32);
                }
            }
        }
        depth += 1;
        current = next;
    }
    depths
}

pub fn co_slice_pdb() -> &'static [u8] {
    static CO_SLICE_PDB: OnceLock<Vec<u8>> = OnceLock::new();
    CO_SLICE_PDB.get_or_init(|| {
        let path = cache_dir().join(CO_SLICE_CACHE_FILE);
        if let Ok(bytes) = fs::read(&path) {
            if bytes.len() == NUM_CO_SLICE_COORDS {
                return bytes;
            }
        }
        let depths = bfs_co_slice_pdb();
        if fs::create_dir_all(cache_dir()).is_ok() {
            let _ = fs::write(&path, &depths);
        }
        depths
    })
}

// ----- Joint EO × slice PDB -----------------------------------------------
//
// 2048 × 495 ≈ 0.97 MB. The classical Kociemba phase-1 heuristic. Used for
// full-HTM scrambles where CP coord is too large to seed quickly.

fn bfs_eo_slice_pdb() -> Vec<u8> {
    let eo_table = eo_move_table();
    let slice_table = slice_move_table();
    const UNREACHED: u8 = 0xff;
    let n = NUM_EO_SLICE_COORDS;
    let mut depths = vec![UNREACHED; n];
    let start = 0 * NUM_SLICE_COORDS + SOLVED_SLICE_COORD as usize;
    depths[start] = 0;
    let mut current: Vec<u32> = vec![start as u32];
    let mut depth = 0u8;
    while !current.is_empty() {
        let mut next: Vec<u32> = Vec::new();
        for &idx in &current {
            let eo = (idx as usize) / NUM_SLICE_COORDS;
            let sl = (idx as usize) % NUM_SLICE_COORDS;
            let eo_row = &eo_table[eo];
            let sl_row = &slice_table[sl];
            for m in 0..18 {
                let new_eo = eo_row[m] as usize;
                let new_sl = sl_row[m] as usize;
                let new_idx = new_eo * NUM_SLICE_COORDS + new_sl;
                if depths[new_idx] == UNREACHED {
                    depths[new_idx] = depth + 1;
                    next.push(new_idx as u32);
                }
            }
        }
        depth += 1;
        current = next;
    }
    depths
}

// ----- Subgroup-aware PDBs --------------------------------------------------
//
// PDBs above are BFS-built with all 18 face moves. For DSL-restricted queries
// like `<R,U,F>` (T-perm gen) the full-HTM PDB underestimates distance — at
// T-perm with CO=EO=0, joint CO×EO returns 0 and CP_PDB returns the full-HTM
// distance (~10), but the actual distance in `<R,U,F>` is 14. The loose LB
// makes IDA* explore many spurious branches.
//
// Subgroup PDBs are BFS in coord space using only the **allowed** moves. For
// any state in the subgroup's orbit of the goal, PDB[coord] is the exact
// minimum number of subgroup moves to solve that axis. For states not in the
// orbit, PDB[coord] = UNREACHED (0xff) — IDA* can prune immediately.
//
// Cache key = `excluded_face_axes` packed into a u8 (bit i = excluded axis i).
// Building a CP subgroup PDB takes <50ms (40320 entries × 6-9 moves). Joint
// subgroup PDBs are larger; we build per-query on demand and memoize.

use std::collections::HashMap;
use std::sync::Mutex;

fn excluded_to_key(excluded: &[bool; 6]) -> u8 {
    let mut k = 0u8;
    for (i, &e) in excluded.iter().enumerate() {
        if e {
            k |= 1 << i;
        }
    }
    k
}

static SUBGROUP_CP_CACHE: Mutex<Option<HashMap<u8, &'static [u8]>>> = Mutex::new(None);
static SUBGROUP_CO_EO_CACHE: Mutex<Option<HashMap<u8, &'static [u8]>>> = Mutex::new(None);
static SUBGROUP_CP_CO_CACHE: Mutex<Option<HashMap<u8, &'static [u8]>>> = Mutex::new(None);

fn bfs_subgroup_cp_pdb(excluded: &[bool; 6]) -> Vec<u8> {
    let move_table = cp_move_table();
    const UNREACHED: u8 = 0xff;
    let mut depths = vec![UNREACHED; NUM_CP_COORDS];
    depths[0] = 0;
    let mut current: Vec<u32> = vec![0];
    let mut depth = 0u8;
    while !current.is_empty() {
        let mut next: Vec<u32> = Vec::new();
        for &coord in &current {
            for axis in 0..6 {
                if excluded[axis] {
                    continue;
                }
                for variant in 0..3 {
                    let m = axis * 3 + variant;
                    let next_coord = move_table[coord as usize][m];
                    if depths[next_coord as usize] == UNREACHED {
                        depths[next_coord as usize] = depth + 1;
                        next.push(next_coord);
                    }
                }
            }
        }
        depth += 1;
        current = next;
    }
    depths
}

/// Return a CP PDB BFS'd over the subgroup generated by the allowed face
/// axes (i.e. excluding those marked `true` in `excluded`). Cached by
/// excluded-axes bitmask. Cache lives for process lifetime; the values are
/// `&'static [u8]` because the PDB is intentionally leaked into a `Box::leak`
/// — each unique restriction is built at most once.
pub fn subgroup_cp_pdb(excluded: &[bool; 6]) -> &'static [u8] {
    let key = excluded_to_key(excluded);
    let mut guard = SUBGROUP_CP_CACHE.lock().unwrap();
    if guard.is_none() {
        *guard = Some(HashMap::new());
    }
    let map = guard.as_mut().unwrap();
    if let Some(&v) = map.get(&key) {
        return v;
    }
    let depths = bfs_subgroup_cp_pdb(excluded);
    let leaked: &'static [u8] = Box::leak(depths.into_boxed_slice());
    map.insert(key, leaked);
    leaked
}

fn bfs_subgroup_co_eo_pdb(excluded: &[bool; 6]) -> Vec<u8> {
    let co_table = co_move_table();
    let eo_table = eo_move_table();
    const UNREACHED: u8 = 0xff;
    let n = NUM_CO_EO_COORDS;
    let mut depths = vec![UNREACHED; n];
    depths[0] = 0;
    let mut current: Vec<u32> = vec![0];
    let mut depth = 0u8;
    while !current.is_empty() {
        let mut next: Vec<u32> = Vec::new();
        for &idx in &current {
            let co = (idx as usize) / NUM_EO_COORDS;
            let eo = (idx as usize) % NUM_EO_COORDS;
            let co_row = &co_table[co];
            let eo_row = &eo_table[eo];
            for axis in 0..6 {
                if excluded[axis] {
                    continue;
                }
                for variant in 0..3 {
                    let m = axis * 3 + variant;
                    let new_co = co_row[m] as usize;
                    let new_eo = eo_row[m] as usize;
                    let new_idx = new_co * NUM_EO_COORDS + new_eo;
                    if depths[new_idx] == UNREACHED {
                        depths[new_idx] = depth + 1;
                        next.push(new_idx as u32);
                    }
                }
            }
        }
        depth += 1;
        current = next;
    }
    depths
}

/// Return a joint CO×EO PDB BFS'd over the subgroup. Same memoization as
/// `subgroup_cp_pdb`. Building one is ~4.5M × {6,9,12,15} transitions, takes
/// 1-3 seconds the first time per unique restriction.
pub fn subgroup_co_eo_pdb(excluded: &[bool; 6]) -> &'static [u8] {
    let key = excluded_to_key(excluded);
    let mut guard = SUBGROUP_CO_EO_CACHE.lock().unwrap();
    if guard.is_none() {
        *guard = Some(HashMap::new());
    }
    let map = guard.as_mut().unwrap();
    if let Some(&v) = map.get(&key) {
        return v;
    }
    let depths = bfs_subgroup_co_eo_pdb(excluded);
    let leaked: &'static [u8] = Box::leak(depths.into_boxed_slice());
    map.insert(key, leaked);
    leaked
}

// ----- Subgroup-aware joint CP × CO PDB ----------------------------------
//
// Plan v5 Phase v5-1 deliverable. The killer LB for subgroup-restricted
// enumerate queries (e.g. T-perm in <R,U,F>). 40320 (CP) × 2187 (CO) =
// 88,099,440 entries ≈ 84 MB per generator-set.
//
// BFS over joint (cp, co) state under only the allowed face moves. For
// T-perm in <R,U,F> this returns exactly 14 at the T-perm state (CP=tperm,
// CO=0), tightening the root IDA* LB from ~10 (full-HTM CP alone) to 14.
//
// Build time: ~5-30s the first time per unique restriction. Cached on-disk
// at %TEMP%/gen-search-pruning-v4/cp_co_subgroup_<excluded_hex>.prun.

fn cp_co_subgroup_cache_file(excluded: &[bool; 6]) -> String {
    let key = excluded_to_key(excluded);
    format!("cp_co_subgroup_{:02x}.prun", key)
}

fn bfs_subgroup_cp_co_pdb(excluded: &[bool; 6]) -> Vec<u8> {
    let cp_table = cp_move_table();
    let co_table = co_move_table();
    const UNREACHED: u8 = 0xff;
    let n = NUM_CP_COORDS * NUM_CO_COORDS;
    let mut depths = vec![UNREACHED; n];
    // Solved state: cp=0, co=0  ->  idx = 0
    depths[0] = 0;
    let mut current: Vec<u32> = vec![0];
    let mut depth = 0u8;
    while !current.is_empty() {
        let mut next: Vec<u32> = Vec::with_capacity(current.len() * 6);
        for &idx in &current {
            let cp = (idx as usize) / NUM_CO_COORDS;
            let co = (idx as usize) % NUM_CO_COORDS;
            let cp_row = &cp_table[cp];
            let co_row = &co_table[co];
            for axis in 0..6 {
                if excluded[axis] {
                    continue;
                }
                for variant in 0..3 {
                    let m = axis * 3 + variant;
                    let new_cp = cp_row[m] as usize;
                    let new_co = co_row[m] as usize;
                    let new_idx = new_cp * NUM_CO_COORDS + new_co;
                    if depths[new_idx] == UNREACHED {
                        depths[new_idx] = depth + 1;
                        next.push(new_idx as u32);
                    }
                }
            }
        }
        depth += 1;
        current = next;
    }
    depths
}

/// Return a joint CP × CO PDB BFS'd over the subgroup generated by the
/// allowed face axes. Memoized by excluded-axes bitmask. Disk-cached so
/// the ~5-30s build cost is paid only once per unique restriction.
///
/// Index: `pdb[cp * NUM_CO_COORDS + co]`. UNREACHED (0xff) means the
/// state is not reachable from solved under the allowed moves — IDA* can
/// prune immediately.
pub fn subgroup_cp_co_pdb(excluded: &[bool; 6]) -> &'static [u8] {
    let key = excluded_to_key(excluded);
    let mut guard = SUBGROUP_CP_CO_CACHE.lock().unwrap();
    if guard.is_none() {
        *guard = Some(HashMap::new());
    }
    let map = guard.as_mut().unwrap();
    if let Some(&v) = map.get(&key) {
        return v;
    }
    // Try disk cache (84 MB; load is much cheaper than rebuild)
    let cache_path = cache_dir().join(cp_co_subgroup_cache_file(excluded));
    let depths: Vec<u8> = if let Ok(bytes) = fs::read(&cache_path) {
        if bytes.len() == NUM_CP_COORDS * NUM_CO_COORDS {
            bytes
        } else {
            let d = bfs_subgroup_cp_co_pdb(excluded);
            if fs::create_dir_all(cache_dir()).is_ok() {
                let _ = fs::write(&cache_path, &d);
            }
            d
        }
    } else {
        let d = bfs_subgroup_cp_co_pdb(excluded);
        if fs::create_dir_all(cache_dir()).is_ok() {
            let _ = fs::write(&cache_path, &d);
        }
        d
    };
    let leaked: &'static [u8] = Box::leak(depths.into_boxed_slice());
    map.insert(key, leaked);
    leaked
}

pub fn eo_slice_pdb() -> &'static [u8] {
    static EO_SLICE_PDB: OnceLock<Vec<u8>> = OnceLock::new();
    EO_SLICE_PDB.get_or_init(|| {
        let path = cache_dir().join(EO_SLICE_CACHE_FILE);
        if let Ok(bytes) = fs::read(&path) {
            if bytes.len() == NUM_EO_SLICE_COORDS {
                return bytes;
            }
        }
        let depths = bfs_eo_slice_pdb();
        if fs::create_dir_all(cache_dir()).is_ok() {
            let _ = fs::write(&path, &depths);
        }
        depths
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cubie_coord::corner_ori_coord;

    #[test]
    fn co_pdb_solved_distance_zero() {
        let pdb = co_pdb();
        assert_eq!(pdb.len(), NUM_CO_COORDS);
        assert_eq!(pdb[0], 0, "solved CO coord must have distance 0");
    }

    #[test]
    fn co_pdb_after_R_distance_one() {
        use crate::cubie::{apply_cubie_move, build_face_moves};
        let moves = build_face_moves();
        // R move (base index 2 = R, variant 0 = CW)
        let r_move = &moves[2 * 3 + 0];
        let after_r = apply_cubie_move(&CubieState::solved(), r_move);
        let (co, _) = corner_ori_coord(&after_r);
        let pdb = co_pdb();
        assert_eq!(pdb[co as usize], 1, "after single R, CO distance = 1");
    }

    #[test]
    fn co_pdb_all_reachable() {
        let pdb = co_pdb();
        let max = pdb.iter().copied().max().unwrap();
        // Tight upper bound for CO alone in HTM is known to be ~10 moves
        assert!(max < 20, "CO PDB max depth ridiculously high: {}", max);
        // Every entry is reached (no 0xff)
        assert!(pdb.iter().all(|&d| d != 0xff));
    }
}
