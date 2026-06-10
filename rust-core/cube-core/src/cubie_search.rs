//! Cubie-coord IDA* iterative deepening search (plan v4 Phase v4-3).
//!
//! Replaces the facelet IDA*+PDB engine. Faster because:
//!   - cubie permutation is 8+12 byte arrays (vs 54-byte facelet); apply_move
//!     is a permutation by index, ~ns per move
//!   - PDB lookup is O(1) by coord (no "enumerate consistent coords" overhead)
//!   - move restriction via `exclude_axis[]` is just a branching filter
//!
//! Current scope (single-threaded, single PDB):
//!   - corner_ori PDB only (Phase v4-2 deliverable)
//!   - face turns only (U/D/R/L/F/B × 3 variants = 18 moves)
//!   - mask-aware goal check (compare each piece slot against solved with
//!     POSITION_UNKNOWN / ORIENTATION_UNKNOWN treated as wildcards)
//!   - DSL `exclude_axis[]` from pattern's bases-union
//!
//! Phase v4-4..v4-6 add multi-thread, SIMD, more PDBs, icase modes.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Instant;

use rayon::prelude::*;

use crate::cubie::{
    apply_cubie_move, build_face_moves, CubieMove, CubieState, EdgeCubie, CornerCubie,
    NUM_CORNERS, NUM_EDGES, ORIENTATION_UNKNOWN, POSITION_UNKNOWN,
};
use crate::cubie_coord::{
    corner_ori_coord, corner_perm_coord, edge_ori_coord, NUM_CO_COORDS, NUM_EO_COORDS,
};
use crate::cubie_pdb::{
    co_eo_pdb, co_pdb, cp_pdb, eo_pdb, subgroup_co_eo_pdb, subgroup_cp_co_pdb, subgroup_cp_pdb,
};

/// Per-axis-variant move-count limit state for DSL `[F*<=3]` / `[F<=2]` /
/// `[F2<=0]` etc. BaseLimits. Each constraint references either a specific
/// (base, variant) or a base-wide group (`variant=None` → all 3 variants
/// counted together). The DFS checks `usages[li] < maxes[li]` before applying
/// a move and reverts on backtrack.
///
/// `limits_by_move[base*3 + variant]` is the per-move dispatch table: the
/// list of constraint indices that this specific (base, variant) move falls
/// under. Built once at dispatch time; immutable during search.
#[derive(Debug, Clone)]
pub struct BaseLimitState {
    pub maxes: Vec<usize>,
    pub usages: Vec<usize>,
    pub limits_by_move: Vec<Vec<usize>>, // [base*3 + variant] -> Vec<limit_idx>
}

impl BaseLimitState {
    /// Build from the DSL pattern's `base_limits` vector. `num_bases` is the
    /// count of base-face axes (= 6 for cubie engine: U/D/R/L/F/B).
    pub fn from_pattern(
        bls: &[crate::dsl::pattern::BaseLimit],
        num_bases: usize,
    ) -> Self {
        let mut maxes = Vec::with_capacity(bls.len());
        let mut usages = Vec::with_capacity(bls.len());
        let mut limits_by_move = vec![Vec::new(); num_bases * 3];
        for l in bls {
            let idx = maxes.len();
            maxes.push(l.max);
            usages.push(0);
            match l.variant {
                None => {
                    for v in 0..3 {
                        if l.base * 3 + v < limits_by_move.len() {
                            limits_by_move[l.base * 3 + v].push(idx);
                        }
                    }
                }
                Some(v) => {
                    if l.base * 3 + v < limits_by_move.len() {
                        limits_by_move[l.base * 3 + v].push(idx);
                    }
                }
            }
        }
        Self {
            maxes,
            usages,
            limits_by_move,
        }
    }

    /// Returns true if applying move `move_idx` (= base*3 + variant) would
    /// not exceed any active limit. Cheap O(L) where L is the number of
    /// constraints touching this move (typically 0-2).
    #[inline]
    pub fn can_apply(&self, move_idx: usize) -> bool {
        if move_idx >= self.limits_by_move.len() {
            return true;
        }
        for &li in &self.limits_by_move[move_idx] {
            if self.usages[li] >= self.maxes[li] {
                return false;
            }
        }
        true
    }

    /// Increment usage counters for all constraints touched by `move_idx`.
    #[inline]
    pub fn apply(&mut self, move_idx: usize) {
        if move_idx >= self.limits_by_move.len() {
            return;
        }
        let ls = self.limits_by_move[move_idx].clone();
        for li in ls {
            self.usages[li] += 1;
        }
    }

    /// Decrement usage counters for all constraints touched by `move_idx`
    /// (called on backtrack).
    #[inline]
    pub fn unapply(&mut self, move_idx: usize) {
        if move_idx >= self.limits_by_move.len() {
            return;
        }
        let ls = self.limits_by_move[move_idx].clone();
        for li in ls {
            self.usages[li] -= 1;
        }
    }
}

/// Per-goal admissibility flags for the PDB lower bounds. Computed once at
/// `try_cubie_dispatch` time and reused throughout DFS.
#[derive(Clone, Copy)]
pub struct LbAdmissibility {
    /// `true` ↔ goal requires every corner to have ori=0 (i.e. state.CO must
    /// hit 0 at goal). Implies the CO_PDB is an admissible LB.
    co: bool,
    /// `true` ↔ goal requires every edge to have ori=0.
    eo: bool,
    /// `true` ↔ goal requires every corner at its solved slot (CP=0).
    cp: bool,
}

impl LbAdmissibility {
    /// Derive admissibility from a (corners, edges) goal pair. Unknown
    /// orientation slots (`ORIENTATION_UNKNOWN`) break admissibility for that
    /// dimension because the state can end with non-zero coord there.
    pub fn from_goal(
        goal_corners: &[CornerCubie; NUM_CORNERS],
        goal_edges: &[EdgeCubie; NUM_EDGES],
    ) -> Self {
        let co = goal_corners.iter().all(|c| c.ori == 0);
        let eo = goal_edges.iter().all(|e| e.ori == 0);
        let cp = goal_corners
            .iter()
            .enumerate()
            .all(|(i, c)| c.pos == i as u8);
        Self { co, eo, cp }
    }
}

/// Single-threaded IDA* iterative deepening on cubie state.
///
/// Returns `(solutions, nodes_explored, elapsed_ms)`. Each solution is a
/// `Vec<usize>` of cube-core move indices (compatible with `formatMoves`).
///
/// # Arguments
/// * `initial` — starting cubie state (may contain `POSITION_UNKNOWN` /
///   `ORIENTATION_UNKNOWN` slots for masked queries).
/// * `goal_corners`, `goal_edges` — per-slot target identity. Unknown slots
///   in goal (`pos == POSITION_UNKNOWN`) are treated as wildcards.
/// * `max_depth` — search up to this many moves inclusive.
/// * `excluded_face_axes` — `[bool; 6]` for U/D/R/L/F/B. If true, that
///   axis is never tried during DFS.
/// * `enumerate` — `false` = stop at first solution found at the first
///   depth that has any (shortest-only). `true` = collect all solutions
///   up to `max_depth`.
/// * `should_cancel` — periodically polled; returns true → abort.
pub fn search_cubie(
    initial: &CubieState,
    goal_corners: &[CornerCubie; NUM_CORNERS],
    goal_edges: &[EdgeCubie; NUM_EDGES],
    max_depth: usize,
    excluded_face_axes: [bool; 6],
    enumerate: bool,
    should_cancel: &mut dyn FnMut() -> bool,
) -> (Vec<Vec<usize>>, usize, u128) {
    search_cubie_ex(
        initial,
        goal_corners,
        goal_edges,
        max_depth,
        0,
        None,
        excluded_face_axes,
        enumerate,
        should_cancel,
    )
}

/// Extended single-threaded entry that accepts BaseLimits and a minimum
/// solution length (= quantifier min on a single-set pattern). v7-1.5
/// addition to let the cubie engine handle DSL `[F*<=3]` etc. and explicit
/// quantifier ranges `<R,U,F>{14,16}` natively, instead of falling through
/// to the legacy facelet engine.
#[allow(clippy::too_many_arguments)]
pub fn search_cubie_ex(
    initial: &CubieState,
    goal_corners: &[CornerCubie; NUM_CORNERS],
    goal_edges: &[EdgeCubie; NUM_EDGES],
    max_depth: usize,
    min_length: usize,
    base_limit_state: Option<BaseLimitState>,
    excluded_face_axes: [bool; 6],
    enumerate: bool,
    should_cancel: &mut dyn FnMut() -> bool,
) -> (Vec<Vec<usize>>, usize, u128) {
    let start = Instant::now();
    let face_moves = build_face_moves();
    let adm = LbAdmissibility::from_goal(goal_corners, goal_edges);
    let pdbs = LbPdbRefs::load(&adm, &excluded_face_axes);
    let mut bl_state = base_limit_state;

    let mut solutions: Vec<Vec<usize>> = Vec::new();
    let mut nodes_explored: usize = 0usize;
    let mut path: Vec<usize> = Vec::new();
    let mut cancelled = false;

    'depth: for depth in 0..=max_depth {
        if cancelled || should_cancel() {
            cancelled = true;
            break;
        }
        // Quick combined PDB lower bound at root
        let lb = combined_lower_bound(initial, &adm, &pdbs);
        if lb > depth {
            continue;
        }
        dfs_search(
            initial,
            goal_corners,
            goal_edges,
            depth,
            -1, // last axis
            &excluded_face_axes,
            &face_moves,
            &adm,
            &pdbs,
            enumerate,
            min_length,
            bl_state.as_mut(),
            &mut path,
            &mut solutions,
            &mut nodes_explored,
            &mut cancelled,
            should_cancel,
        );
        if !solutions.is_empty() && !enumerate {
            break 'depth;
        }
    }
    let elapsed = start.elapsed().as_millis();
    (solutions, nodes_explored, elapsed)
}

/// Bundle of PDB references picked by goal admissibility. Loading is gated
/// so we never pay the build cost for PDBs we won't query.
///
/// When `excluded_axes` has any true entry (DSL restricted to a subgroup),
/// the **subgroup-aware** PDB (BFS over only the allowed moves) is used
/// instead of the full-HTM PDB. The full-HTM PDB underestimates distance
/// in a restricted subgroup (e.g. T-perm in `<R,U,F>` has full-HTM CP
/// distance ~10 but subgroup distance 14), causing wasted IDA* exploration.
struct LbPdbRefs {
    co_eo: Option<&'static [u8]>,
    co: Option<&'static [u8]>,
    eo: Option<&'static [u8]>,
    cp: Option<&'static [u8]>,
    /// Joint CP × CO PDB. When both `adm.co` and `adm.cp` hold, this gives a
    /// strictly tighter LB than max(co_pdb, cp_pdb) and is the killer
    /// optimization for subgroup-restricted enumerate queries (plan v5-1).
    cp_co: Option<&'static [u8]>,
}

impl LbPdbRefs {
    fn load(adm: &LbAdmissibility, excluded_axes: &[bool; 6]) -> Self {
        let dsl_restricted = excluded_axes.iter().any(|&b| b);
        let co_eo = if adm.co && adm.eo {
            Some(if dsl_restricted {
                subgroup_co_eo_pdb(excluded_axes)
            } else {
                co_eo_pdb()
            })
        } else {
            None
        };
        let co = if adm.co && !adm.eo { Some(co_pdb()) } else { None };
        let eo = if adm.eo && !adm.co { Some(eo_pdb()) } else { None };
        let cp = if adm.cp {
            Some(if dsl_restricted {
                subgroup_cp_pdb(excluded_axes)
            } else {
                cp_pdb()
            })
        } else {
            None
        };
        // Joint CP × CO: only used when both axes are admissible. Restricted
        // DSL gets the subgroup-aware variant (the whole point of plan v5-1).
        // Full HTM defers to the unjoint cp + co_eo combination for now (no
        // full-HTM joint cp_co built into v5; future work).
        let cp_co = if adm.co && adm.cp && dsl_restricted {
            Some(subgroup_cp_co_pdb(excluded_axes))
        } else {
            None
        };
        Self {
            co_eo,
            co,
            eo,
            cp,
            cp_co,
        }
    }
}

/// Admissible HTM lower bound. The selected PDB(s) depend on which axes the
/// goal forces (`LbAdmissibility`). Joint CO×EO PDB is preferred when both
/// axes are forced — it dominates max(co_pdb, eo_pdb) and produces ~2x node
/// reduction on hard OLL cases.
fn combined_lower_bound(
    state: &CubieState,
    adm: &LbAdmissibility,
    pdbs: &LbPdbRefs,
) -> usize {
    let mut lb: usize = 0;
    if adm.co && adm.eo {
        let (co, co_unk) = corner_ori_coord(state);
        let (eo, eo_unk) = edge_ori_coord(state);
        if co_unk == 0 && eo_unk == 0 {
            let idx = (co as usize) * NUM_EO_COORDS + eo as usize;
            lb = lb.max(pdbs.co_eo.unwrap()[idx] as usize);
        }
    } else if adm.co {
        let (co, co_unk) = corner_ori_coord(state);
        if co_unk == 0 {
            lb = lb.max(pdbs.co.unwrap()[co as usize] as usize);
        }
    } else if adm.eo {
        let (eo, eo_unk) = edge_ori_coord(state);
        if eo_unk == 0 {
            lb = lb.max(pdbs.eo.unwrap()[eo as usize] as usize);
        }
    }
    if adm.cp {
        let (cp, cp_unk) = corner_perm_coord(state);
        if cp_unk == 0 {
            lb = lb.max(pdbs.cp.unwrap()[cp as usize] as usize);
            // Joint cp_co (subgroup-aware) is strictly tighter than max(cp, co)
            // alone when both axes are admissible (plan v5-1).
            if let Some(cp_co) = pdbs.cp_co {
                let (co, co_unk) = corner_ori_coord(state);
                if co_unk == 0 {
                    let idx = (cp as usize) * NUM_CO_COORDS + co as usize;
                    let v = cp_co[idx] as usize;
                    // 0xff (UNREACHED) -> state is unreachable from solved
                    // under the allowed move set; treat as infinity (no
                    // valid solution from here within any depth).
                    if v < 0xff {
                        lb = lb.max(v);
                    } else {
                        return usize::MAX;
                    }
                }
            }
        }
    }
    lb
}

#[allow(clippy::too_many_arguments)]
fn dfs_search(
    state: &CubieState,
    goal_corners: &[CornerCubie; NUM_CORNERS],
    goal_edges: &[EdgeCubie; NUM_EDGES],
    remaining: usize,
    last_axis: i32,
    excluded_face_axes: &[bool; 6],
    face_moves: &[CubieMove],
    adm: &LbAdmissibility,
    pdbs: &LbPdbRefs,
    enumerate: bool,
    min_length: usize,
    base_limit_state: Option<&mut BaseLimitState>,
    path: &mut Vec<usize>,
    solutions: &mut Vec<Vec<usize>>,
    nodes_explored: &mut usize,
    cancelled: &mut bool,
    should_cancel: &mut dyn FnMut() -> bool,
) {
    if *cancelled {
        return;
    }
    if *nodes_explored & 0xffff == 0 && should_cancel() {
        *cancelled = true;
        return;
    }
    *nodes_explored += 1;

    // Enumerate mode: check goal at every depth (find solutions of any
    // length 0..=max_depth in a single DFS). Iterative deepening would
    // re-explore the depth-k prefix tree for every iteration and at this
    // engine's per-node cost that's a 2x penalty (~16s vs 9s on T-perm
    // RUF gen). The caller sorts by length before emit so the UI still
    // sees solutions in length order.
    //
    // ShortestOnly mode: only check at remaining==0; the parallel loop
    // does iterative deepening 0..max_depth and stops at the first depth
    // that yields a solution.
    //
    // `min_length` (= quantifier min on a single-set pattern) gates emit:
    // a candidate must have path.len() >= min_length to be accepted.
    if enumerate && path.len() >= min_length && is_goal(state, goal_corners, goal_edges) {
        solutions.push(path.clone());
    }
    if remaining == 0 {
        if !enumerate && path.len() >= min_length && is_goal(state, goal_corners, goal_edges) {
            solutions.push(path.clone());
        }
        return;
    }

    // Combined PDB lower bound prune
    let lb = combined_lower_bound(state, adm, pdbs);
    if lb > remaining {
        return;
    }

    // Try each of 6 face axes
    let mut bl_ref = base_limit_state;
    for axis in 0..6 {
        if excluded_face_axes[axis] {
            continue;
        }
        // No two consecutive moves on same axis (face_axis = axis/1 for face
        // turns; pair U/D, R/L, F/B as same "physical axis" for canonicality)
        let physical_axis = axis / 2; // U,D -> 0; R,L -> 1; F,B -> 2
        if last_axis as i32 == physical_axis as i32 {
            // Allow only if axis > last_axis_specific (canonical order U<D, R<L, F<B)
            // Skip to avoid duplicate sequences
            continue;
        }
        // base index in our face_moves: U=0, D=1, R=2, L=3, F=4, B=5
        let base_idx = axis;
        for variant in 0..3 {
            let move_idx_cubie = base_idx * 3 + variant; // index in face_moves Vec
            // cube-core move index (for path emission): same mapping
            let cube_core_move_idx = base_idx * 3 + variant;

            // BaseLimit check: skip if this move would exceed any active
            // constraint (e.g. `[F*<=3]` blocks any F-axis move after 3 uses).
            if let Some(bl) = bl_ref.as_deref() {
                if !bl.can_apply(move_idx_cubie) {
                    continue;
                }
            }

            let new_state = apply_cubie_move(state, &face_moves[move_idx_cubie]);
            path.push(cube_core_move_idx);
            if let Some(bl) = bl_ref.as_deref_mut() {
                bl.apply(move_idx_cubie);
            }
            dfs_search(
                &new_state,
                goal_corners,
                goal_edges,
                remaining - 1,
                physical_axis as i32,
                excluded_face_axes,
                face_moves,
                adm,
                pdbs,
                enumerate,
                min_length,
                bl_ref.as_deref_mut(),
                path,
                solutions,
                nodes_explored,
                cancelled,
                should_cancel,
            );
            if let Some(bl) = bl_ref.as_deref_mut() {
                bl.unapply(move_idx_cubie);
            }
            path.pop();
            if !enumerate && !solutions.is_empty() {
                return;
            }
            if *cancelled {
                return;
            }
        }
    }
}

/// Mask-aware goal check: each slot in `state` must match `goal` (unknown
/// goal slots are wildcards). Used by IDA* terminal test.
fn is_goal(
    state: &CubieState,
    goal_corners: &[CornerCubie; NUM_CORNERS],
    goal_edges: &[EdgeCubie; NUM_EDGES],
) -> bool {
    for slot in 0..NUM_CORNERS {
        let g = goal_corners[slot];
        if g.pos == POSITION_UNKNOWN {
            continue;
        }
        let s = state.corners[slot];
        if s.pos != g.pos {
            return false;
        }
        if g.ori != ORIENTATION_UNKNOWN && s.ori != g.ori {
            return false;
        }
    }
    for slot in 0..NUM_EDGES {
        let g = goal_edges[slot];
        if g.pos == POSITION_UNKNOWN {
            continue;
        }
        let s = state.edges[slot];
        if s.pos != g.pos {
            return false;
        }
        if g.ori != ORIENTATION_UNKNOWN && s.ori != g.ori {
            return false;
        }
    }
    true
}

/// Multi-thread variant of `search_cubie`. Each iterative-deepening depth
/// is searched with the 6 face axes parallelized across rayon threads.
///
/// Shortest-only mode: the first solution found at any depth atomically
/// sets `found_at_depth`; other threads check this and short-circuit if a
/// solution already exists at the same depth.
///
/// Returns `(solutions, total_nodes_explored, elapsed_ms)`.
pub fn search_cubie_parallel(
    initial: &CubieState,
    goal_corners: &[CornerCubie; NUM_CORNERS],
    goal_edges: &[EdgeCubie; NUM_EDGES],
    max_depth: usize,
    excluded_face_axes: [bool; 6],
    enumerate: bool,
    should_cancel: &(dyn Fn() -> bool + Sync),
) -> (Vec<Vec<usize>>, usize, u128) {
    search_cubie_parallel_ex(
        initial,
        goal_corners,
        goal_edges,
        max_depth,
        0,
        None,
        excluded_face_axes,
        enumerate,
        should_cancel,
    )
}

/// Multi-thread variant with BaseLimits + min_length (= quantifier min on a
/// single-set pattern) support. v7-1.5: lets cubie engine handle DSL
/// constraints natively instead of falling through to the legacy facelet
/// engine on `[F*<=3]` / `<R,U,F>{14,16}` queries.
///
/// Each axis-thread gets its own clone of `base_limit_state` (= per-thread
/// usage counters), so there is no shared mutable state and no lock
/// contention beyond the existing `solutions_mu`.
#[allow(clippy::too_many_arguments)]
pub fn search_cubie_parallel_ex(
    initial: &CubieState,
    goal_corners: &[CornerCubie; NUM_CORNERS],
    goal_edges: &[EdgeCubie; NUM_EDGES],
    max_depth: usize,
    min_length: usize,
    base_limit_state: Option<BaseLimitState>,
    excluded_face_axes: [bool; 6],
    enumerate: bool,
    should_cancel: &(dyn Fn() -> bool + Sync),
) -> (Vec<Vec<usize>>, usize, u128) {
    let start = Instant::now();
    let face_moves = build_face_moves();
    let adm = LbAdmissibility::from_goal(goal_corners, goal_edges);
    let pdbs = LbPdbRefs::load(&adm, &excluded_face_axes);
    // Cloned per axis-thread below.
    let bl_template = base_limit_state;

    let solutions_mu: Mutex<Vec<Vec<usize>>> = Mutex::new(Vec::new());
    let total_nodes = AtomicUsize::new(0);
    let cancelled = AtomicBool::new(false);

    // Enumerate mode: single DFS to max_depth with goal-check at every
    // level (see `dfs_search`). Solutions of every length 0..=max_depth
    // get collected in one tree walk.
    // ShortestOnly mode: iterative deepening 0, 1, ..., max_depth — stops
    // at the first depth that yields any solution.
    let depth_range: Vec<usize> = if enumerate {
        vec![max_depth]
    } else {
        (0..=max_depth).collect()
    };

    for &depth in &depth_range {
        if cancelled.load(Ordering::Relaxed) || should_cancel() {
            cancelled.store(true, Ordering::Relaxed);
            break;
        }
        let lb = combined_lower_bound(initial, &adm, &pdbs);
        if lb > depth {
            continue;
        }

        let found_this_depth = AtomicBool::new(false);

        // Parallelize over the 6 face axes at the root. Each axis-thread
        // tries 3 variants × subtree DFS. Each axis-thread clones the
        // base-limit template so per-thread usage counters are isolated.
        (0..6usize).into_par_iter().for_each(|axis| {
            if excluded_face_axes[axis] {
                return;
            }
            if cancelled.load(Ordering::Relaxed) {
                return;
            }
            if !enumerate && found_this_depth.load(Ordering::Relaxed) {
                return;
            }
            if depth == 0 {
                // Special case: no moves applied; checked by serial path at depth=0.
                return;
            }
            let physical_axis = axis / 2;
            for variant in 0..3 {
                if !enumerate && found_this_depth.load(Ordering::Relaxed) {
                    return;
                }
                if cancelled.load(Ordering::Relaxed) {
                    return;
                }
                let mv_idx = axis * 3 + variant;
                // BaseLimit check at root: clone the template per axis-thread,
                // pre-apply the root move's counters. If the root move itself
                // exceeds a limit, skip it (e.g. `[F*<=0]` with axis=F).
                let mut local_bl = bl_template.clone();
                if let Some(bl) = local_bl.as_mut() {
                    if !bl.can_apply(mv_idx) {
                        continue;
                    }
                    bl.apply(mv_idx);
                }
                let new_state = apply_cubie_move(initial, &face_moves[mv_idx]);
                let mut local_solutions: Vec<Vec<usize>> = Vec::new();
                let mut local_nodes: usize = 0;
                let mut local_path: Vec<usize> = vec![mv_idx];
                let mut local_cancelled = false;
                let mut cancel_fn = || cancelled.load(Ordering::Relaxed) || should_cancel();
                dfs_search(
                    &new_state,
                    goal_corners,
                    goal_edges,
                    depth - 1,
                    physical_axis as i32,
                    &excluded_face_axes,
                    &face_moves,
                    &adm,
                    &pdbs,
                    enumerate,
                    min_length,
                    local_bl.as_mut(),
                    &mut local_path,
                    &mut local_solutions,
                    &mut local_nodes,
                    &mut local_cancelled,
                    &mut cancel_fn,
                );
                total_nodes.fetch_add(local_nodes, Ordering::Relaxed);
                if !local_solutions.is_empty() {
                    found_this_depth.store(true, Ordering::Relaxed);
                    let mut guard = solutions_mu.lock().unwrap();
                    guard.extend(local_solutions);
                }
            }
        });

        // Depth 0 (or first enumerate pass): check empty path. dfs_search
        // is only called after applying one move at the parallel root, so
        // it never sees the initial state itself.
        // Empty solution (length 0) only valid if min_length is 0.
        if (depth == 0 || enumerate) && min_length == 0 {
            if is_goal(initial, goal_corners, goal_edges) {
                let mut guard = solutions_mu.lock().unwrap();
                guard.push(Vec::new());
                if !enumerate {
                    break;
                }
            }
        }

        if !enumerate && !solutions_mu.lock().unwrap().is_empty() {
            break;
        }
    }
    let elapsed = start.elapsed().as_millis();
    let solutions = solutions_mu.into_inner().unwrap();
    let total = total_nodes.load(Ordering::Relaxed);
    (solutions, total, elapsed)
}

/// Build per-slot goal cubies from a goal facelet `(Facelet)` and a mask
/// `(Mask)`. Slots whose facelet stickers are all masked-out (WILDCARD)
/// become `POSITION_UNKNOWN` (wildcard match).
pub fn build_goal_from_facelet_and_mask(
    goal_facelet: &crate::facelet::Facelet,
    mask: &crate::mask::Mask,
) -> ([CornerCubie; NUM_CORNERS], [EdgeCubie; NUM_EDGES]) {
    // First bake the mask into a goal facelet so cubie identification knows
    // which slots have full info.
    let masked = crate::mask::bake_in_mask(goal_facelet, mask);
    let cubie_goal = crate::cubie::facelet_to_cubie(&masked);
    (cubie_goal.corners, cubie_goal.edges)
}

/// Construct a cubie goal for a named preset directly (avoids the
/// "partial info" ambiguity of facelet→cubie when only some stickers of a
/// cubie are visible). Returns `None` for unknown preset ids.
///
/// Recognized presets:
///   - "OLL":  U-layer pieces oriented (ori=0), positions free; lower 2 layers solved
///   - "PLL":  full solve
///   - "F2L":  lower 2 layers solved; U-layer completely free
///   - "CMLL"/"COLL":  same as PLL for now (corner orient+perm; conservative)
///   - "OCLL": U-corners oriented (ori=0), positions free; rest solved
///   - "EOLL": U-edges oriented (ori=0), positions free; rest solved
pub fn cubie_goal_from_preset(
    preset_id: &str,
) -> Option<([CornerCubie; NUM_CORNERS], [EdgeCubie; NUM_EDGES])> {
    let solved = CubieState::solved();
    let mut corners = solved.corners;
    let mut edges = solved.edges;

    let u_corner_slots = [0usize, 1, 2, 3]; // URF, UFL, ULB, UBR
    let u_edge_slots = [0usize, 1, 2, 3]; // UR, UF, UL, UB

    match preset_id {
        "OLL" => {
            for &c in &u_corner_slots {
                corners[c] = CornerCubie {
                    pos: POSITION_UNKNOWN,
                    ori: 0,
                };
            }
            for &e in &u_edge_slots {
                edges[e] = EdgeCubie {
                    pos: POSITION_UNKNOWN,
                    ori: 0,
                };
            }
        }
        "PLL" | "CMLL" | "COLL" => {
            // Identity goal — full solve
        }
        "F2L" => {
            for &c in &u_corner_slots {
                corners[c] = CornerCubie {
                    pos: POSITION_UNKNOWN,
                    ori: ORIENTATION_UNKNOWN,
                };
            }
            for &e in &u_edge_slots {
                edges[e] = EdgeCubie {
                    pos: POSITION_UNKNOWN,
                    ori: ORIENTATION_UNKNOWN,
                };
            }
        }
        "OCLL" => {
            // U-corners oriented (ori=0), positions free; rest solved
            for &c in &u_corner_slots {
                corners[c] = CornerCubie {
                    pos: POSITION_UNKNOWN,
                    ori: 0,
                };
            }
        }
        "EOLL" => {
            // U-edges oriented (ori=0), positions free; rest solved
            for &e in &u_edge_slots {
                edges[e] = EdgeCubie {
                    pos: POSITION_UNKNOWN,
                    ori: 0,
                };
            }
        }
        _ => return None,
    }
    Some((corners, edges))
}

/// High-level dispatcher: try to handle a `SearchRequest` with the cubie
/// engine. Returns:
///   - `Some(SearchStats)` on success
///   - `None` if cubie engine doesn't apply — caller should fall back to
///     the legacy facelet engine
///
/// Currently dispatches to cubie when:
///   - the request's mask + preset combination has a known cubie goal
///   - the DSL uses only face bases (U/D/R/L/F/B — no wide/slice/rotation)
///   - `output_mode == ShortestOnly` (Enumerate fallback TBD)
///   - `top_n.is_none()` (TopN fallback TBD)
pub fn try_cubie_dispatch(
    req: &crate::search::SearchRequest,
    on_solution: Option<&mut dyn FnMut(usize, &[usize])>,
    should_cancel: &(dyn Fn() -> bool + Sync),
) -> Option<crate::search::SearchStats> {
    use crate::dsl::parser::parse_pattern;
    use crate::facelet::FACELET_COUNT;
    use crate::moves::move_base;
    use crate::search::{OutputMode, SearchStats};

    if req.initial.len() != FACELET_COUNT || req.mask.len() != FACELET_COUNT {
        return None;
    }

    // Parse pattern to compute excludeAxis and check non-face moves
    let pattern = parse_pattern(&req.dsl).ok()?;
    use crate::dsl::pattern::PatternElement;
    for el in &pattern.elements {
        let bases: Vec<usize> = match el {
            PatternElement::Literal { move_idx, .. } => vec![move_base(*move_idx)],
            PatternElement::Set { allowed_bases, .. } => allowed_bases.clone(),
        };
        if bases.iter().any(|&b| b >= 6) {
            // wide / slice / rotation — cubie engine doesn't handle yet
            return None;
        }
    }
    // Multi-element / literal patterns (e.g. `<U> F <R,U> F' <U>`) carry
    // *positional* constraints that the cubie engine's `excludeAxis`-based
    // collapse loses. Fall back to the legacy facelet engine which compiles
    // patterns element-by-element with per-position move tables.
    // Single-set patterns (with or without quantifier range) keep the cubie
    // path because there is no positional structure to lose.
    let single_set = pattern.elements.len() == 1
        && matches!(&pattern.elements[0], PatternElement::Set { .. });
    if !single_set {
        return None;
    }
    // BaseLimits: cubie engine handles these via DFS counters (v7-1.5).
    let base_limit_state = pattern
        .base_limits
        .as_ref()
        .filter(|bl| !bl.is_empty())
        .map(|bls| BaseLimitState::from_pattern(bls, 6));

    // Quantifier with explicit range: cubie engine handles the single Set
    // pattern's `{min, max}` natively (v7-1.5):
    //   - `min` → only emit solutions of length ≥ min
    //   - `max` → cap effective search depth at `min(max_moves, set_max)`
    // `single_set` is guaranteed by the early-bail above.
    let (min_length, set_max_override) = match &pattern.elements[0] {
        PatternElement::Set { min, max, .. } => (*min, *max),
        _ => unreachable!("single_set guarantee"),
    };

    let excluded = exclude_axis_from_pattern(&pattern);
    let dsl_restricted = excluded.iter().any(|&b| b);

    // Construct initial cubie state from physical facelet (mask positions
    // filled with solved colors to give cubie identifier something to read,
    // matching CE Incomp's NNN-substitution approach).
    let mut facelet: crate::facelet::Facelet = [0i8; FACELET_COUNT];
    for i in 0..FACELET_COUNT {
        let v = req.initial[i];
        facelet[i] = if v < 0 || v > 5 { (i / 9) as i8 } else { v };
    }
    let initial = crate::cubie::facelet_to_cubie(&facelet);

    // Determine cubie goal from mask pattern. Strategy:
    //   - all_visible + DSL unrestricted (`<U,D,R,L,F,B>`): fall through to
    //     legacy's min2phase oracle. It gets a tight upper bound in ms and
    //     IDA* shortest-proof inherits it. The cubie engine here has no
    //     edge-perm PDB so its LB is too loose for full-HTM random scrambles.
    //   - all_visible + DSL restricted (e.g. `<R,U,F>`): min2phase doesn't
    //     apply (it requires full HTM). Cubie engine with joint CO×EO + CP
    //     PDBs handles this well — the excluded_face_axes prune branches
    //     directly at the DFS level (CE-compatible). This is the T-perm RU
    //     gen / RUF gen / etc. use case.
    //   - Known preset mask: use the preset's cubie goal.
    //   - Custom partial mask: fall back to legacy.
    let all_visible = req.mask.iter().all(|&m| m == 1);
    let (goal_corners, goal_edges) = if all_visible {
        // Full-mask queries:
        //   - Unrestricted DSL (`<U,D,R,L,F,B>`): legacy's min2phase oracle
        //     gives a fast upper bound. Defer.
        //   - Restricted DSL (`<R,U,F>` etc.): use cubie with the full set
        //     of subgroup-aware PDBs (subgroup_cp + subgroup_co_eo +
        //     subgroup_cp_co). The joint cp_co PDB (plan v5-1) gives root
        //     LB = exact subgroup distance, eliminating wasted exploration
        //     at depths < true_distance. Works for both ShortestOnly and
        //     Enumerate output modes.
        if !dsl_restricted {
            return None;
        }
        let solved = CubieState::solved();
        (solved.corners, solved.edges)
    } else {
        match recognize_preset_mask(&req.mask) {
            Some(preset_id) => cubie_goal_from_preset(preset_id)?,
            None => {
                return None;
            }
        }
    };

    // ShortestOnly: stop at first depth with solutions (enumerate=false).
    // Enumerate/TopN: collect all solutions up to max_moves (enumerate=true).
    let enumerate = !matches!(req.output_mode, OutputMode::ShortestOnly);

    let cancel_arc = should_cancel;
    // Effective search depth: cap by quantifier max (single-set patterns).
    let effective_max_depth = set_max_override
        .map(|m| m.min(req.max_moves))
        .unwrap_or(req.max_moves);
    let (sols, nodes, ms) = search_cubie_parallel_ex(
        &initial,
        &goal_corners,
        &goal_edges,
        effective_max_depth,
        min_length,
        base_limit_state,
        excluded,
        enumerate,
        cancel_arc,
    );

    // Enumerate/TopN: sort by length so the UI sees shortest-first. The
    // single-DFS Enumerate path collects in DFS order, not length order.
    let mut sols = sols;
    match req.output_mode {
        OutputMode::EnumerateUpToN => {
            sols.sort_by_key(|s| s.len());
        }
        OutputMode::TopN => {
            sols.sort_by_key(|s| s.len());
            if let Some(n) = req.top_n {
                sols.truncate(n);
            }
        }
        _ => {}
    }

    // Emit solutions. ShortestOnly emits all the shortest-tied results;
    // Enumerate/TopN emits everything in the collected order.
    let shortest_len = sols.iter().map(|s| s.len()).min();
    if let Some(cb) = on_solution {
        match req.output_mode {
            OutputMode::ShortestOnly => {
                if let Some(min_len) = shortest_len {
                    for sol in &sols {
                        if sol.len() == min_len {
                            cb(sol.len(), sol);
                        }
                    }
                }
            }
            _ => {
                for sol in &sols {
                    cb(sol.len(), sol);
                }
            }
        }
    }

    Some(SearchStats {
        total_solutions: sols.len(),
        time_ms: ms,
        nodes_explored: nodes,
        shortest_length: shortest_len,
    })
}

/// Recognize a known preset id from a 54-byte mask. Returns `None` if the
/// mask doesn't match any registered preset.
fn recognize_preset_mask(mask: &[u8]) -> Option<&'static str> {
    use crate::presets::{mask_from_preset_id};
    for id in ["OLL", "PLL", "F2L", "CMLL", "COLL", "EOLL", "OCLL"] {
        if let Some(p) = mask_from_preset_id(id) {
            let same = (0..54).all(|i| (p[i] != 0) == (mask[i] != 0));
            if same {
                return Some(id);
            }
        }
    }
    None
}

/// Compute `excludeAxis[6]` from a `Pattern`'s bases-union. If a face base
/// never appears anywhere in the pattern, mark it excluded.
pub fn exclude_axis_from_pattern(pattern: &crate::dsl::pattern::Pattern) -> [bool; 6] {
    use crate::dsl::pattern::PatternElement;
    use crate::moves::move_base;
    let mut allowed = [false; 6];
    for el in &pattern.elements {
        match el {
            PatternElement::Literal { move_idx, .. } => {
                let b = move_base(*move_idx);
                if b < 6 {
                    allowed[b] = true;
                }
            }
            PatternElement::Set { allowed_bases, .. } => {
                for &b in allowed_bases {
                    if b < 6 {
                        allowed[b] = true;
                    }
                }
            }
        }
    }
    let mut exclude = [false; 6];
    for i in 0..6 {
        exclude[i] = !allowed[i];
    }
    exclude
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cubie::{facelet_to_cubie, NUM_CORNERS, NUM_EDGES};
    use crate::facelet::create_solved_facelet;
    use crate::moves::{apply_moves, parse_move_token};

    fn scramble_to_moves(s: &str) -> Vec<usize> {
        s.split_whitespace().filter_map(parse_move_token).collect()
    }

    fn solved_goal() -> ([CornerCubie; NUM_CORNERS], [EdgeCubie; NUM_EDGES]) {
        let solved = CubieState::solved();
        (solved.corners, solved.edges)
    }

    #[test]
    fn solves_solved_state_at_depth_zero() {
        let initial = CubieState::solved();
        let (gc, ge) = solved_goal();
        let mut cancel = || false;
        let (sols, _, _) = search_cubie(&initial, &gc, &ge, 0, [false; 6], false, &mut cancel);
        assert_eq!(sols.len(), 1);
        assert_eq!(sols[0].len(), 0);
    }

    #[test]
    fn solves_sune_in_seven_moves() {
        let solved_f = create_solved_facelet();
        let sune = scramble_to_moves("R U R' U R U2 R'");
        let scrambled_f = apply_moves(&solved_f, &sune);
        let initial = facelet_to_cubie(&scrambled_f);
        let (gc, ge) = solved_goal();
        let mut cancel = || false;
        let (sols, nodes, ms) =
            search_cubie(&initial, &gc, &ge, 8, [false; 6], false, &mut cancel);
        eprintln!("Sune: {} solutions in {} nodes / {} ms", sols.len(), nodes, ms);
        assert!(!sols.is_empty(), "expected at least one solution");
        let shortest = sols.iter().map(|s| s.len()).min().unwrap();
        assert!(shortest <= 7, "shortest > 7: {}", shortest);
    }

    #[test]
    fn ru_restriction_solves_sune_state() {
        // OLL Sune state should be solvable with <R, U> only (it IS Sune)
        let solved_f = create_solved_facelet();
        let sune = scramble_to_moves("R U R' U R U2 R'");
        let scrambled_f = apply_moves(&solved_f, &sune);
        let initial = facelet_to_cubie(&scrambled_f);
        let (gc, ge) = solved_goal();
        // Exclude D, L, F, B (keep only R/U via face_axes [U=0, D=1, R=2, L=3, F=4, B=5])
        let mut exclude = [false; 6];
        exclude[1] = true; // D
        exclude[3] = true; // L
        exclude[4] = true; // F
        exclude[5] = true; // B
        let mut cancel = || false;
        let (sols, nodes, ms) =
            search_cubie(&initial, &gc, &ge, 8, exclude, false, &mut cancel);
        eprintln!("Sune via <R,U>: {} sols / {} nodes / {} ms", sols.len(), nodes, ms);
        assert!(!sols.is_empty(), "Sune state must be solvable with <R, U>");
    }
}
