//! Pattern-driven search engine.
//!
//! Faithful port of `core/search/engine.ts`. The DFS itself is implemented as
//! an explicit recursive function (the TS version is also recursive — the
//! transposition table and per-depth stacks line up 1:1 with that design).

use std::collections::HashMap;
use std::time::Instant;

use serde::{Deserialize, Serialize};

use crate::coord::{
    encode_corner_perm_from_state, enumerate_corner_ori_coords, enumerate_edge_ori_coords,
    enumerate_slice_coords, get_move_tables, NUM_CO_COORDS, NUM_CP_COORDS, NUM_EO_COORDS,
    NUM_SLICE_COORDS,
};
use crate::dsl::parser::expand_set_moves;
use crate::dsl::pattern::{Pattern, PatternElement};
use crate::facelet::{Facelet, FACELET_COUNT};
use crate::mask::{EffectiveFacelet, WILDCARD};
use crate::moves::{move_base, move_perm, AXIS_OF_BASE, BASE_NAMES};
use crate::rotations::is_solved_rotation_tolerant;
use crate::pruning::get_pruning_tables;
use crate::subgroup::{
    check_corner_orbit_at_state, check_piece_orbits_at_state, compute_corner_orbit,
    compute_piece_orbits, compute_subgroup_invariants, SubgroupInvariants, GOAL_CO, GOAL_CP,
    GOAL_EO, GOAL_SLICE,
};

pub use crate::output_mode::OutputMode;

#[derive(Debug, Clone)]
pub struct SearchInput {
    pub initial: EffectiveFacelet,
    pub goal: Facelet,
    pub pattern: Pattern,
    pub max_moves: usize,
    pub output_mode: OutputMode,
    pub top_n: Option<usize>,
    pub use_pruning_table: bool,
}

#[derive(Debug, Clone)]
pub struct SearchSolution {
    pub moves: Vec<usize>,
    pub length: usize,
}

#[derive(Debug, Clone)]
pub struct SearchProgress {
    pub depth: usize,
    pub solutions_found: usize,
    pub nodes_explored: usize,
    pub elapsed_ms: u128,
}

#[derive(Debug, Clone)]
pub struct SearchStats {
    pub total_solutions: usize,
    pub time_ms: u128,
    pub nodes_explored: usize,
    pub shortest_length: Option<usize>,
}

pub struct SearchCallbacks {
    pub on_solution: Option<Box<dyn FnMut(&SearchSolution) + Send + 'static>>,
    pub on_progress: Option<Box<dyn FnMut(&SearchProgress) + Send + 'static>>,
    pub should_cancel: Option<Box<dyn FnMut() -> bool + Send + 'static>>,
}

impl Default for SearchCallbacks {
    fn default() -> Self {
        Self {
            on_solution: None,
            on_progress: None,
            should_cancel: None,
        }
    }
}

#[derive(Clone)]
enum Compiled {
    Literal {
        move_idx: usize,
    },
    Set {
        allowed_moves: Vec<usize>,
        min: usize,
        max: usize,
    },
}

fn max_stickers_fixed_per_move_for_base(base: usize) -> usize {
    // Wide moves touch two layers (24 stickers). Rotations relabel the whole
    // cube so every sticker position changes (54). Otherwise (face / slice)
    // one layer = 12 stickers.
    if base >= 6 && base < 12 {
        24
    } else if base >= 15 {
        54
    } else {
        12
    }
}

const TRANSPOSITION_CAP: usize = 2_000_000;
const MAX_CAND_PRODUCT: usize = 4096;

fn compile(pattern: &Pattern, max_moves: usize) -> Vec<Compiled> {
    pattern
        .elements
        .iter()
        .map(|el| match el {
            PatternElement::Literal { move_idx, .. } => Compiled::Literal { move_idx: *move_idx },
            PatternElement::Set {
                allowed_bases,
                min,
                max,
                ..
            } => {
                let allowed = expand_set_moves(allowed_bases);
                let max_v = match max {
                    None => max_moves,
                    Some(m) => (*m).min(max_moves),
                };
                Compiled::Set {
                    allowed_moves: allowed,
                    min: (*min).min(max_v),
                    max: max_v,
                }
            }
        })
        .collect()
}

fn compute_remaining_min(compiled: &[Compiled]) -> Vec<usize> {
    let n = compiled.len();
    let mut out = vec![0usize; n + 1];
    for i in (0..n).rev() {
        let cost = match &compiled[i] {
            Compiled::Literal { .. } => 1,
            Compiled::Set { min, .. } => *min,
        };
        out[i] = out[i + 1] + cost;
    }
    out
}

fn compute_suffix_invariants(compiled: &[Compiled]) -> Vec<SubgroupInvariants> {
    let n = compiled.len();
    let mut suffix: Vec<SubgroupInvariants> = Vec::with_capacity(n + 1);
    let mut acc = std::collections::BTreeSet::<usize>::new();
    let mut tmp: Vec<SubgroupInvariants> = vec![compute_subgroup_invariants(&[]); n + 1];
    for k in (0..n).rev() {
        match &compiled[k] {
            Compiled::Literal { move_idx } => {
                acc.insert(move_base(*move_idx));
            }
            Compiled::Set { allowed_moves, .. } => {
                for &m in allowed_moves {
                    acc.insert(move_base(m));
                }
            }
        }
        let bases: Vec<usize> = acc.iter().copied().collect();
        tmp[k] = compute_subgroup_invariants(&bases);
    }
    for s in tmp.drain(..) {
        suffix.push(s);
    }
    suffix
}

fn hash_key(
    state: &EffectiveFacelet,
    element_idx: usize,
    last_axis: i32,
    last_base: i32,
) -> String {
    let mut s = String::with_capacity(30);
    // Header chars carry namespacing bits to avoid collisions with the body.
    s.push(char::from_u32(((element_idx as u32) & 0xff) | 0x100).unwrap());
    s.push(char::from_u32((((last_axis as i32) & 0xff) as u32) | 0x200).unwrap());
    s.push(char::from_u32((((last_base as i32) & 0xff) as u32) | 0x300).unwrap());
    let mut i = 0;
    while i < FACELET_COUNT {
        let a = ((state[i] + 1) & 0xf) as u32;
        let b = ((state[i + 1] + 1) & 0xf) as u32;
        s.push(char::from_u32((a << 4) | b).unwrap());
        i += 2;
    }
    s
}

/// Main search entry point. Mirrors `runSearch` in `engine.ts`.
///
/// Hybrid routing (per plan v3 + INCIDENTS.md INC-2026-05-18-01):
/// 1. impossibility pre-check (Level 1: basis-union subgroup orbit BFS, kept
///    here at the head of `run_search` body to share state with IDA* legs)
/// 2. min2phase oracle (NEVER emits) — provides upper bound + short-circuits
///    full-HTM unrestricted "no solution" cases
/// 3. PDB lower-bound short-circuit if min2phase candidate equals LB
/// 4. mask-aware IDA*+PDB iterative deepening — proves shortest constructively
///    and emits in depth order (CE incomplete cube filter mode equivalent)
///
/// The actual hybrid logic lives in `routing::run_search_hybrid`; this
/// function is the public entry point and delegates immediately.
pub fn run_search(input: &SearchInput, cb: SearchCallbacks) -> SearchStats {
    crate::search::routing::run_search_hybrid(input, cb)
}

/// Underlying IDA*+PDB iterative deepening (the old `run_search` body).
/// Called by the routing layer either directly (constraints incompatible
/// with min2phase) or with an upper bound from the min2phase oracle.
pub(crate) fn run_ida_star(input: &SearchInput, mut cb: SearchCallbacks) -> SearchStats {
    let start = Instant::now();
    let compiled = compile(&input.pattern, input.max_moves);
    let remaining_min = compute_remaining_min(&compiled);
    let suffix_invariants = compute_suffix_invariants(&compiled);
    let element_has_invariants: Vec<bool> = suffix_invariants
        .iter()
        .take(compiled.len())
        .map(|s| s.has_any())
        .collect();
    let max_moves = input.max_moves;
    let output_mode = input.output_mode;
    let top_n = input.top_n.unwrap_or(usize::MAX);
    let goal = input.goal;

    // Piece-orbit (Lv1+Lv2) subgroup membership check at the root. The
    // pattern's union basis defines a subgroup <B>; if the initial state is
    // not in <B>·solved, no search depth can solve it — return early with
    // zero nodes. Skips when the basis covers all 6 face axes (trivial group).
    {
        let mut all_bases: std::collections::BTreeSet<usize> =
            std::collections::BTreeSet::new();
        for el in &compiled {
            match el {
                Compiled::Literal { move_idx } => {
                    all_bases.insert(move_base(*move_idx));
                }
                Compiled::Set { allowed_moves, .. } => {
                    for &m in allowed_moves {
                        all_bases.insert(move_base(m));
                    }
                }
            }
        }
        let basis_vec: Vec<usize> = all_bases.iter().copied().collect();
        // Goal-side rotation tolerance: a state already rotation-tolerantly
        // solved must never be rejected — let the regular path emit it.
        let rot_solved = is_solved_rotation_tolerant(&input.initial, &goal);

        // Lv1+Lv2 piece-pair check: cheap, catches single-piece and pairwise
        // impossibilities (e.g. DLF non-home under <R, U>).
        let pair_orbits = compute_piece_orbits(&basis_vec);
        if !pair_orbits.is_full_group
            && !rot_solved
            && !check_piece_orbits_at_state(&input.initial, &pair_orbits)
        {
            return SearchStats {
                total_solutions: 0,
                time_ms: start.elapsed().as_millis(),
                nodes_explored: 0,
                shortest_length: None,
            };
        }

        // Corner-state orbit BFS: strictly stronger than pair checks.
        // Catches 3-piece-correlation impossibilities like (URF UFL UBR)
        // 3-cycle with CO=0 under <R, U>.
        let corner_orbit = compute_corner_orbit(&basis_vec);
        if !corner_orbit.is_full_group
            && !rot_solved
            && !check_corner_orbit_at_state(&input.initial, &corner_orbit)
        {
            return SearchStats {
                total_solutions: 0,
                time_ms: start.elapsed().as_millis(),
                nodes_explored: 0,
                shortest_length: None,
            };
        }
    }

    // Base-limit handling
    let num_bases = BASE_NAMES.len();
    let mut limit_maxes: Vec<usize> = Vec::new();
    let mut limit_usages: Vec<usize> = Vec::new();
    let mut limits_by_move: Vec<Vec<usize>> = vec![Vec::new(); num_bases * 3];
    if let Some(bl) = &input.pattern.base_limits {
        for l in bl {
            let idx = limit_maxes.len();
            limit_maxes.push(l.max);
            limit_usages.push(0);
            match l.variant {
                None => {
                    for v in 0..3 {
                        limits_by_move[l.base * 3 + v].push(idx);
                    }
                }
                Some(v) => {
                    limits_by_move[l.base * 3 + v].push(idx);
                }
            }
        }
    }
    let has_limits = !limit_maxes.is_empty();

    // maxFixPerMove
    let mut max_fix_per_move = 12usize;
    for el in &compiled {
        match el {
            Compiled::Literal { move_idx } => {
                let v = max_stickers_fixed_per_move_for_base(move_base(*move_idx));
                if v > max_fix_per_move {
                    max_fix_per_move = v;
                }
            }
            Compiled::Set { allowed_moves, .. } => {
                for &m in allowed_moves {
                    let v = max_stickers_fixed_per_move_for_base(move_base(m));
                    if v > max_fix_per_move {
                        max_fix_per_move = v;
                    }
                }
            }
        }
    }

    // Coord heuristic setup
    let mut use_coord = false;
    let mut k_co = 0usize;
    let mut k_eo = 0usize;
    let mut k_sl = 0usize;
    let mut co_cands0: Vec<u32> = Vec::new();
    let mut eo_cands0: Vec<u32> = Vec::new();
    let mut sl_cands0: Vec<u32> = Vec::new();
    let mut co_table: Option<&[u8]> = None;
    let mut eo_table: Option<&[u8]> = None;
    let mut co_eo_table: Option<&[u8]> = None;
    let mut cp_co_table: Option<&[u8]> = None;
    let mut co_move: Option<&[u32]> = None;
    let mut eo_move: Option<&[u32]> = None;
    let mut slice_move: Option<&[u32]> = None;
    let mut cp_move: Option<&[u32]> = None;
    let mut cp0: i32 = -1;

    if input.use_pruning_table {
        let co_set = enumerate_corner_ori_coords(&input.initial);
        let eo_set = enumerate_edge_ori_coords(&input.initial);
        let sl_set = enumerate_slice_coords(&input.initial);
        if !co_set.is_empty()
            && !eo_set.is_empty()
            && !sl_set.is_empty()
            && co_set.len() * sl_set.len() <= MAX_CAND_PRODUCT
            && eo_set.len() * sl_set.len() <= MAX_CAND_PRODUCT
            && co_set.len() * eo_set.len() <= MAX_CAND_PRODUCT
        {
            let p = get_pruning_tables();
            co_table = Some(&p.co_slice);
            eo_table = Some(&p.eo_slice);
            co_eo_table = Some(&p.co_eo);
            let mt = get_move_tables();
            co_move = Some(&mt.co);
            eo_move = Some(&mt.eo);
            slice_move = Some(&mt.slice);
            cp0 = encode_corner_perm_from_state(&input.initial);
            if cp0 >= 0 {
                cp_co_table = Some(&p.cp_co);
                cp_move = Some(&mt.cp);
            }
            k_co = co_set.len();
            k_eo = eo_set.len();
            k_sl = sl_set.len();
            co_cands0 = co_set;
            eo_cands0 = eo_set;
            sl_cands0 = sl_set;
            use_coord = true;
        }
    }

    // Stacks
    let mut state_stack: Vec<EffectiveFacelet> = vec![[0i8; FACELET_COUNT]; max_moves + 1];
    state_stack[0] = input.initial;
    let mut mismatch_stack = vec![0i32; max_moves + 1];

    let mut co_stack: Vec<u32> = if use_coord {
        vec![0u32; (max_moves + 1) * k_co]
    } else {
        Vec::new()
    };
    let mut eo_stack: Vec<u32> = if use_coord {
        vec![0u32; (max_moves + 1) * k_eo]
    } else {
        Vec::new()
    };
    let mut slice_stack: Vec<u32> = if use_coord {
        vec![0u32; (max_moves + 1) * k_sl]
    } else {
        Vec::new()
    };
    let mut cp_stack: Vec<u32> = if cp_co_table.is_some() {
        vec![0u32; max_moves + 1]
    } else {
        Vec::new()
    };

    if use_coord {
        for i in 0..k_co {
            co_stack[i] = co_cands0[i];
        }
        for i in 0..k_eo {
            eo_stack[i] = eo_cands0[i];
        }
        for i in 0..k_sl {
            slice_stack[i] = sl_cands0[i];
        }
    }
    if cp_co_table.is_some() {
        cp_stack[0] = cp0 as u32;
    }

    {
        let mut m = 0i32;
        for i in 0..FACELET_COUNT {
            let v = state_stack[0][i];
            if v != WILDCARD && v != goal[i] {
                m += 1;
            }
        }
        mismatch_stack[0] = m;
    }

    let use_transposition = !matches!(output_mode, OutputMode::EnumerateUpToN) && !has_limits;
    let mut transposition: Option<HashMap<String, usize>> =
        if use_transposition { Some(HashMap::new()) } else { None };

    // Iterative deepening
    let global_min = remaining_min[0];
    let mut solutions_found = 0usize;
    let mut nodes_explored = 0usize;
    let mut shortest_length: Option<usize> = None;
    let mut cancelled = false;
    let mut aborted = false;
    let mut path: Vec<usize> = Vec::new();

    let move_perm_ref = move_perm();

    'outer: for d in global_min..=max_moves {
        if cancelled || aborted {
            break;
        }
        if let Some(cb_p) = cb.on_progress.as_deref_mut() {
            let p = SearchProgress {
                depth: d,
                solutions_found,
                nodes_explored,
                elapsed_ms: start.elapsed().as_millis(),
            };
            cb_p(&p);
        }
        let solutions_at_start = solutions_found;

        // Run dfs at target depth d
        run_dfs(
            d,
            true,
            &compiled,
            &remaining_min,
            &suffix_invariants,
            &element_has_invariants,
            &goal,
            move_perm_ref,
            &mut state_stack,
            &mut mismatch_stack,
            &mut co_stack,
            &mut eo_stack,
            &mut slice_stack,
            &mut cp_stack,
            k_co,
            k_eo,
            k_sl,
            use_coord,
            co_table,
            eo_table,
            co_eo_table,
            cp_co_table,
            co_move,
            eo_move,
            slice_move,
            cp_move,
            max_fix_per_move,
            has_limits,
            &mut limit_usages,
            &limit_maxes,
            &limits_by_move,
            &mut path,
            &mut solutions_found,
            &mut nodes_explored,
            &mut shortest_length,
            &mut cancelled,
            &mut aborted,
            output_mode,
            top_n,
            &mut transposition,
            &mut cb,
        );

        if matches!(output_mode, OutputMode::ShortestOnly) && solutions_found > solutions_at_start {
            break 'outer;
        }
        if matches!(output_mode, OutputMode::TopN) && solutions_found >= top_n {
            break 'outer;
        }
    }

    SearchStats {
        total_solutions: solutions_found,
        time_ms: start.elapsed().as_millis(),
        nodes_explored,
        shortest_length,
    }
}

fn apply_move_and_mismatch(
    depth: usize,
    m: usize,
    move_perm_ref: &[[i8; FACELET_COUNT]],
    state_stack: &mut [EffectiveFacelet],
    mismatch_stack: &mut [i32],
    co_stack: &mut [u32],
    eo_stack: &mut [u32],
    slice_stack: &mut [u32],
    cp_stack: &mut [u32],
    k_co: usize,
    k_eo: usize,
    k_sl: usize,
    use_coord: bool,
    cp_co_table_is_some: bool,
    co_move: Option<&[u32]>,
    eo_move: Option<&[u32]>,
    slice_move: Option<&[u32]>,
    cp_move: Option<&[u32]>,
    goal: &Facelet,
) -> i32 {
    let perm = &move_perm_ref[m];
    let (left, right) = state_stack.split_at_mut(depth + 1);
    let old_state = &left[depth];
    let new_state = &mut right[0];
    let mut mism = 0i32;
    for i in 0..FACELET_COUNT {
        let v = old_state[perm[i] as usize];
        new_state[i] = v;
        if v != WILDCARD && v != goal[i] {
            mism += 1;
        }
    }
    mismatch_stack[depth + 1] = mism;

    // Coord update is uniform across all 54 moves: face/wide/slice (0..45)
    // and whole-cube rotations x/y/z (45..54). The pruning BFS (cache v2)
    // includes rotation transitions, so the heuristic remains admissible
    // even when rotations are in the path.
    if use_coord {
        let co_mv = co_move.unwrap();
        let eo_mv = eo_move.unwrap();
        let sl_mv = slice_move.unwrap();
        let co_base = depth * k_co;
        let co_next = co_base + k_co;
        let co_row = m * NUM_CO_COORDS;
        for i in 0..k_co {
            co_stack[co_next + i] = co_mv[co_row + co_stack[co_base + i] as usize];
        }
        let eo_base = depth * k_eo;
        let eo_next = eo_base + k_eo;
        let eo_row = m * NUM_EO_COORDS;
        for i in 0..k_eo {
            eo_stack[eo_next + i] = eo_mv[eo_row + eo_stack[eo_base + i] as usize];
        }
        let sl_base = depth * k_sl;
        let sl_next = sl_base + k_sl;
        let sl_row = m * NUM_SLICE_COORDS;
        for i in 0..k_sl {
            slice_stack[sl_next + i] = sl_mv[sl_row + slice_stack[sl_base + i] as usize];
        }
    }
    if cp_co_table_is_some {
        let cp_mv = cp_move.unwrap();
        cp_stack[depth + 1] = cp_mv[m * NUM_CP_COORDS + cp_stack[depth] as usize];
    }
    mism
}

fn lower_bound(
    depth: usize,
    mismatch: i32,
    state_stack: &[EffectiveFacelet],
    goal: &Facelet,
    co_stack: &[u32],
    eo_stack: &[u32],
    slice_stack: &[u32],
    cp_stack: &[u32],
    k_co: usize,
    k_eo: usize,
    k_sl: usize,
    co_table: Option<&[u8]>,
    eo_table: Option<&[u8]>,
    co_eo_table: Option<&[u8]>,
    cp_co_table: Option<&[u8]>,
    max_fix_per_move: usize,
) -> i32 {
    if let (Some(co_t), Some(eo_t), Some(co_eo_t)) = (co_table, eo_table, co_eo_table) {
        let mut min_a = 255u8;
        let mut min_b = 255u8;
        let mut min_c = 255u8;
        let co_base = depth * k_co;
        let eo_base = depth * k_eo;
        let sl_base = depth * k_sl;
        for i in 0..k_co {
            let co_val = co_stack[co_base + i] as usize;
            let co_slice_idx = co_val * NUM_SLICE_COORDS;
            for j in 0..k_sl {
                let v = co_t[co_slice_idx + slice_stack[sl_base + j] as usize];
                if v < min_a {
                    min_a = v;
                }
            }
            let co_eo_idx = co_val * NUM_EO_COORDS;
            for j in 0..k_eo {
                let v = co_eo_t[co_eo_idx + eo_stack[eo_base + j] as usize];
                if v < min_c {
                    min_c = v;
                }
            }
        }
        for i in 0..k_eo {
            let eo_idx = (eo_stack[eo_base + i] as usize) * NUM_SLICE_COORDS;
            for j in 0..k_sl {
                let v = eo_t[eo_idx + slice_stack[sl_base + j] as usize];
                if v < min_b {
                    min_b = v;
                }
            }
        }
        let mut h = min_a;
        if min_b > h {
            h = min_b;
        }
        if min_c > h {
            h = min_c;
        }
        if let Some(cp_t) = cp_co_table {
            let cp_co_idx = (cp_stack[depth] as usize) * NUM_CO_COORDS;
            let mut min_d = 255u8;
            for i in 0..k_co {
                let v = cp_t[cp_co_idx + co_stack[co_base + i] as usize];
                if v < min_d {
                    min_d = v;
                }
            }
            if min_d > h {
                h = min_d;
            }
        }
        return h as i32;
    }
    if mismatch > 0 {
        // Mismatch fallback (pruning tables disabled or unavailable). The
        // raw mismatch count is canonical-frame-only; if the state is
        // physically solved in some rotated frame, treat it as already at
        // the goal so the rotation-tolerant terminal check downstream isn't
        // pruned away.
        if is_solved_rotation_tolerant(&state_stack[depth], goal) {
            return 0;
        }
        ((mismatch as usize + max_fix_per_move - 1) / max_fix_per_move) as i32
    } else {
        0
    }
}

fn check_subgroup_invariants(
    element_idx: usize,
    moves_used: usize,
    suffix_invariants: &[SubgroupInvariants],
    state_stack: &[EffectiveFacelet],
    goal: &Facelet,
    use_coord: bool,
    co_stack: &[u32],
    eo_stack: &[u32],
    slice_stack: &[u32],
    cp_stack: &[u32],
    k_co: usize,
    k_eo: usize,
    k_sl: usize,
    cp_co_table_is_some: bool,
) -> bool {
    let inv = &suffix_invariants[element_idx];
    let state = &state_stack[moves_used];
    for &p in &inv.facelet_positions {
        let v = state[p];
        if v != WILDCARD && v != goal[p] {
            return false;
        }
    }
    if !use_coord {
        return true;
    }
    if inv.preserves_co && k_co > 0 {
        let base = moves_used * k_co;
        let mut ok = false;
        for i in 0..k_co {
            if co_stack[base + i] == GOAL_CO {
                ok = true;
                break;
            }
        }
        if !ok {
            return false;
        }
    }
    if inv.preserves_eo && k_eo > 0 {
        let base = moves_used * k_eo;
        let mut ok = false;
        for i in 0..k_eo {
            if eo_stack[base + i] == GOAL_EO {
                ok = true;
                break;
            }
        }
        if !ok {
            return false;
        }
    }
    if inv.preserves_slice && k_sl > 0 {
        let base = moves_used * k_sl;
        let mut ok = false;
        for i in 0..k_sl {
            if slice_stack[base + i] == GOAL_SLICE {
                ok = true;
                break;
            }
        }
        if !ok {
            return false;
        }
    }
    if inv.preserves_cp && cp_co_table_is_some {
        if cp_stack[moves_used] != GOAL_CP {
            return false;
        }
    }
    true
}

#[allow(clippy::too_many_arguments)]
fn run_dfs(
    target_depth: usize,
    exact: bool,
    compiled: &[Compiled],
    remaining_min: &[usize],
    suffix_invariants: &[SubgroupInvariants],
    element_has_invariants: &[bool],
    goal: &Facelet,
    move_perm_ref: &[[i8; FACELET_COUNT]],
    state_stack: &mut [EffectiveFacelet],
    mismatch_stack: &mut [i32],
    co_stack: &mut [u32],
    eo_stack: &mut [u32],
    slice_stack: &mut [u32],
    cp_stack: &mut [u32],
    k_co: usize,
    k_eo: usize,
    k_sl: usize,
    use_coord: bool,
    co_table: Option<&[u8]>,
    eo_table: Option<&[u8]>,
    co_eo_table: Option<&[u8]>,
    cp_co_table: Option<&[u8]>,
    co_move: Option<&[u32]>,
    eo_move: Option<&[u32]>,
    slice_move: Option<&[u32]>,
    cp_move: Option<&[u32]>,
    max_fix_per_move: usize,
    has_limits: bool,
    limit_usages: &mut [usize],
    limit_maxes: &[usize],
    limits_by_move: &[Vec<usize>],
    path: &mut Vec<usize>,
    solutions_found: &mut usize,
    nodes_explored: &mut usize,
    shortest_length: &mut Option<usize>,
    cancelled: &mut bool,
    aborted: &mut bool,
    output_mode: OutputMode,
    top_n: usize,
    transposition: &mut Option<HashMap<String, usize>>,
    cb: &mut SearchCallbacks,
) {
    let cp_co_some = cp_co_table.is_some();
    let initial_mism = mismatch_stack[0];
    dfs_node(
        DfsCtx {
            target_depth,
            exact,
            compiled,
            remaining_min,
            suffix_invariants,
            element_has_invariants,
            goal,
            move_perm_ref,
            state_stack,
            mismatch_stack,
            co_stack,
            eo_stack,
            slice_stack,
            cp_stack,
            k_co,
            k_eo,
            k_sl,
            use_coord,
            co_table,
            eo_table,
            co_eo_table,
            cp_co_table,
            cp_co_some,
            co_move,
            eo_move,
            slice_move,
            cp_move,
            max_fix_per_move,
            has_limits,
            limit_usages,
            limit_maxes,
            limits_by_move,
            path,
            solutions_found,
            nodes_explored,
            shortest_length,
            cancelled,
            aborted,
            output_mode,
            top_n,
            transposition,
            cb,
        },
        0,
        0,
        -1,
        -1,
        initial_mism,
    );
}

struct DfsCtx<'a> {
    target_depth: usize,
    exact: bool,
    compiled: &'a [Compiled],
    remaining_min: &'a [usize],
    suffix_invariants: &'a [SubgroupInvariants],
    element_has_invariants: &'a [bool],
    goal: &'a Facelet,
    move_perm_ref: &'a [[i8; FACELET_COUNT]],
    state_stack: &'a mut [EffectiveFacelet],
    mismatch_stack: &'a mut [i32],
    co_stack: &'a mut [u32],
    eo_stack: &'a mut [u32],
    slice_stack: &'a mut [u32],
    cp_stack: &'a mut [u32],
    k_co: usize,
    k_eo: usize,
    k_sl: usize,
    use_coord: bool,
    co_table: Option<&'a [u8]>,
    eo_table: Option<&'a [u8]>,
    co_eo_table: Option<&'a [u8]>,
    cp_co_table: Option<&'a [u8]>,
    cp_co_some: bool,
    co_move: Option<&'a [u32]>,
    eo_move: Option<&'a [u32]>,
    slice_move: Option<&'a [u32]>,
    cp_move: Option<&'a [u32]>,
    max_fix_per_move: usize,
    has_limits: bool,
    limit_usages: &'a mut [usize],
    limit_maxes: &'a [usize],
    limits_by_move: &'a [Vec<usize>],
    path: &'a mut Vec<usize>,
    solutions_found: &'a mut usize,
    nodes_explored: &'a mut usize,
    shortest_length: &'a mut Option<usize>,
    cancelled: &'a mut bool,
    aborted: &'a mut bool,
    output_mode: OutputMode,
    top_n: usize,
    transposition: &'a mut Option<HashMap<String, usize>>,
    cb: &'a mut SearchCallbacks,
}

fn dfs_node(
    ctx: DfsCtx<'_>,
    element_idx: usize,
    moves_used: usize,
    last_axis: i32,
    last_base: i32,
    mismatch: i32,
) {
    let DfsCtx {
        target_depth,
        exact,
        compiled,
        remaining_min,
        suffix_invariants,
        element_has_invariants,
        goal,
        move_perm_ref,
        state_stack,
        mismatch_stack,
        co_stack,
        eo_stack,
        slice_stack,
        cp_stack,
        k_co,
        k_eo,
        k_sl,
        use_coord,
        co_table,
        eo_table,
        co_eo_table,
        cp_co_table,
        cp_co_some,
        co_move,
        eo_move,
        slice_move,
        cp_move,
        max_fix_per_move,
        has_limits,
        limit_usages,
        limit_maxes,
        limits_by_move,
        path,
        solutions_found,
        nodes_explored,
        shortest_length,
        cancelled,
        aborted,
        output_mode,
        top_n,
        transposition,
        cb,
    } = ctx;

    if *cancelled || *aborted {
        return;
    }
    if let Some(c) = cb.should_cancel.as_deref_mut() {
        if c() {
            *cancelled = true;
            return;
        }
    }
    let remaining = (target_depth - moves_used) as i32;
    let h = lower_bound(
        moves_used,
        mismatch,
        state_stack,
        goal,
        co_stack,
        eo_stack,
        slice_stack,
        cp_stack,
        k_co,
        k_eo,
        k_sl,
        co_table,
        eo_table,
        co_eo_table,
        cp_co_table,
        max_fix_per_move,
    );
    if h > remaining {
        return;
    }
    // NOTE: subgroup-invariant pruning is conditional on the canonical-frame
    // goal. With the rotation-tolerant goal semantics (state matches any of
    // 24 rotated-solveds), invariants would have to be checked against ALL
    // 24 rotations to stay correct. Until that's implemented, fall back to
    // canonical pruning only when the state isn't rotation-tolerantly solved
    // — the rotated-solved state itself must never be pruned away here.
if element_idx < compiled.len()
        && element_has_invariants[element_idx]
        && !is_solved_rotation_tolerant(&state_stack[moves_used], goal)
        && !check_subgroup_invariants(
            element_idx,
            moves_used,
            suffix_invariants,
            state_stack,
            goal,
            use_coord,
            co_stack,
            eo_stack,
            slice_stack,
            cp_stack,
            k_co,
            k_eo,
            k_sl,
            cp_co_some,
        )
    {
        return;
    }
    if let Some(map) = transposition.as_mut() {
        if remaining >= 3 {
            let key = hash_key(&state_stack[moves_used], element_idx, last_axis, last_base);
            if let Some(&prev_budget) = map.get(&key) {
                if prev_budget >= remaining as usize {
                    return;
                }
            }
            map.insert(key, remaining as usize);
            if map.len() >= TRANSPOSITION_CAP {
                *transposition = None;
            }
        }
    }
    if element_idx == compiled.len() {
        let length_ok = if exact { moves_used == target_depth } else { true };
        // Rotation-tolerant goal: physical solved cube held in any of 24
        // orientations counts as solved. `mismatch == 0` is the fast path
        // (canonical frame); the rotation walk catches the rotated cases.
        if length_ok && (mismatch == 0 || is_solved_rotation_tolerant(&state_stack[moves_used], goal)) {
            emit(
                moves_used,
                path,
                solutions_found,
                shortest_length,
                cb,
                output_mode,
                top_n,
                aborted,
            );
        }
        return;
    }
    if moves_used + remaining_min[element_idx] > target_depth {
        return;
    }

    let el = compiled[element_idx].clone();
    match el {
        Compiled::Literal { move_idx } => {
            let base = move_base(move_idx);
            if has_limits {
                let ls = &limits_by_move[move_idx];
                for &li in ls {
                    if limit_usages[li] >= limit_maxes[li] {
                        return;
                    }
                }
                for &li in ls {
                    limit_usages[li] += 1;
                }
                let axis = AXIS_OF_BASE[base] as i32;
                *nodes_explored += 1;
                path.push(move_idx);
                let new_mism = apply_move_and_mismatch(
                    moves_used,
                    move_idx,
                    move_perm_ref,
                    state_stack,
                    mismatch_stack,
                    co_stack,
                    eo_stack,
                    slice_stack,
                    cp_stack,
                    k_co,
                    k_eo,
                    k_sl,
                    use_coord,
                    cp_co_some,
                    co_move,
                    eo_move,
                    slice_move,
                    cp_move,
                    goal,
                );
                dfs_node(
                    DfsCtx {
                        target_depth,
                        exact,
                        compiled,
                        remaining_min,
                        suffix_invariants,
                        element_has_invariants,
                        goal,
                        move_perm_ref,
                        state_stack,
                        mismatch_stack,
                        co_stack,
                        eo_stack,
                        slice_stack,
                        cp_stack,
                        k_co,
                        k_eo,
                        k_sl,
                        use_coord,
                        co_table,
                        eo_table,
                        co_eo_table,
                        cp_co_table,
                        cp_co_some,
                        co_move,
                        eo_move,
                        slice_move,
                        cp_move,
                        max_fix_per_move,
                        has_limits,
                        limit_usages,
                        limit_maxes,
                        limits_by_move,
                        path,
                        solutions_found,
                        nodes_explored,
                        shortest_length,
                        cancelled,
                        aborted,
                        output_mode,
                        top_n,
                        transposition,
                        cb,
                    },
                    element_idx + 1,
                    moves_used + 1,
                    axis,
                    base as i32,
                    new_mism,
                );
                path.pop();
                for &li in ls {
                    limit_usages[li] -= 1;
                }
                return;
            }
            let axis = AXIS_OF_BASE[base] as i32;
            *nodes_explored += 1;
            path.push(move_idx);
            let new_mism = apply_move_and_mismatch(
                moves_used,
                move_idx,
                move_perm_ref,
                state_stack,
                mismatch_stack,
                co_stack,
                eo_stack,
                slice_stack,
                cp_stack,
                k_co,
                k_eo,
                k_sl,
                use_coord,
                cp_co_some,
                co_move,
                eo_move,
                slice_move,
                cp_move,
                goal,
            );
            dfs_node(
                DfsCtx {
                    target_depth,
                    exact,
                    compiled,
                    remaining_min,
                    suffix_invariants,
                    element_has_invariants,
                    goal,
                    move_perm_ref,
                    state_stack,
                    mismatch_stack,
                    co_stack,
                    eo_stack,
                    slice_stack,
                    cp_stack,
                    k_co,
                    k_eo,
                    k_sl,
                    use_coord,
                    co_table,
                    eo_table,
                    co_eo_table,
                    cp_co_table,
                    cp_co_some,
                    co_move,
                    eo_move,
                    slice_move,
                    cp_move,
                    max_fix_per_move,
                    has_limits,
                    limit_usages,
                    limit_maxes,
                    limits_by_move,
                    path,
                    solutions_found,
                    nodes_explored,
                    shortest_length,
                    cancelled,
                    aborted,
                    output_mode,
                    top_n,
                    transposition,
                    cb,
                },
                element_idx + 1,
                moves_used + 1,
                axis,
                base as i32,
                new_mism,
            );
            path.pop();
        }
        Compiled::Set {
            allowed_moves,
            min,
            max,
        } => {
            let max_len = max.min(target_depth - moves_used);
            enumerate_set_variable_length(
                &allowed_moves,
                min,
                max_len,
                moves_used,
                last_axis,
                last_base,
                mismatch,
                element_idx + 1,
                EnumCtx {
                    target_depth,
                    exact,
                    compiled,
                    remaining_min,
                    suffix_invariants,
                    element_has_invariants,
                    goal,
                    move_perm_ref,
                    state_stack,
                    mismatch_stack,
                    co_stack,
                    eo_stack,
                    slice_stack,
                    cp_stack,
                    k_co,
                    k_eo,
                    k_sl,
                    use_coord,
                    co_table,
                    eo_table,
                    co_eo_table,
                    cp_co_table,
                    cp_co_some,
                    co_move,
                    eo_move,
                    slice_move,
                    cp_move,
                    max_fix_per_move,
                    has_limits,
                    limit_usages,
                    limit_maxes,
                    limits_by_move,
                    path,
                    solutions_found,
                    nodes_explored,
                    shortest_length,
                    cancelled,
                    aborted,
                    output_mode,
                    top_n,
                    transposition,
                    cb,
                },
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn emit(
    length: usize,
    path: &mut Vec<usize>,
    solutions_found: &mut usize,
    shortest_length: &mut Option<usize>,
    cb: &mut SearchCallbacks,
    output_mode: OutputMode,
    top_n: usize,
    aborted: &mut bool,
) {
    *solutions_found += 1;
    if shortest_length.is_none() || length < shortest_length.unwrap() {
        *shortest_length = Some(length);
    }
    if let Some(s) = cb.on_solution.as_deref_mut() {
        let sol = SearchSolution {
            moves: path.clone(),
            length,
        };
        s(&sol);
    }
    if matches!(output_mode, OutputMode::TopN) && *solutions_found >= top_n {
        *aborted = true;
    }
}

struct EnumCtx<'a> {
    target_depth: usize,
    exact: bool,
    compiled: &'a [Compiled],
    remaining_min: &'a [usize],
    suffix_invariants: &'a [SubgroupInvariants],
    element_has_invariants: &'a [bool],
    goal: &'a Facelet,
    move_perm_ref: &'a [[i8; FACELET_COUNT]],
    state_stack: &'a mut [EffectiveFacelet],
    mismatch_stack: &'a mut [i32],
    co_stack: &'a mut [u32],
    eo_stack: &'a mut [u32],
    slice_stack: &'a mut [u32],
    cp_stack: &'a mut [u32],
    k_co: usize,
    k_eo: usize,
    k_sl: usize,
    use_coord: bool,
    co_table: Option<&'a [u8]>,
    eo_table: Option<&'a [u8]>,
    co_eo_table: Option<&'a [u8]>,
    cp_co_table: Option<&'a [u8]>,
    cp_co_some: bool,
    co_move: Option<&'a [u32]>,
    eo_move: Option<&'a [u32]>,
    slice_move: Option<&'a [u32]>,
    cp_move: Option<&'a [u32]>,
    max_fix_per_move: usize,
    has_limits: bool,
    limit_usages: &'a mut [usize],
    limit_maxes: &'a [usize],
    limits_by_move: &'a [Vec<usize>],
    path: &'a mut Vec<usize>,
    solutions_found: &'a mut usize,
    nodes_explored: &'a mut usize,
    shortest_length: &'a mut Option<usize>,
    cancelled: &'a mut bool,
    aborted: &'a mut bool,
    output_mode: OutputMode,
    top_n: usize,
    transposition: &'a mut Option<HashMap<String, usize>>,
    cb: &'a mut SearchCallbacks,
}

#[allow(clippy::too_many_arguments)]
fn enumerate_set_variable_length(
    allowed: &[usize],
    min_len: usize,
    max_len: usize,
    moves_used: usize,
    prev_axis: i32,
    prev_base: i32,
    mismatch: i32,
    then_element_idx: usize,
    mut ctx: EnumCtx<'_>,
) {
    if min_len == 0 {
        // immediate transition with the existing state
        dfs_node(
            DfsCtx {
                target_depth: ctx.target_depth,
                exact: ctx.exact,
                compiled: ctx.compiled,
                remaining_min: ctx.remaining_min,
                suffix_invariants: ctx.suffix_invariants,
                element_has_invariants: ctx.element_has_invariants,
                goal: ctx.goal,
                move_perm_ref: ctx.move_perm_ref,
                state_stack: ctx.state_stack,
                mismatch_stack: ctx.mismatch_stack,
                co_stack: ctx.co_stack,
                eo_stack: ctx.eo_stack,
                slice_stack: ctx.slice_stack,
                cp_stack: ctx.cp_stack,
                k_co: ctx.k_co,
                k_eo: ctx.k_eo,
                k_sl: ctx.k_sl,
                use_coord: ctx.use_coord,
                co_table: ctx.co_table,
                eo_table: ctx.eo_table,
                co_eo_table: ctx.co_eo_table,
                cp_co_table: ctx.cp_co_table,
                cp_co_some: ctx.cp_co_some,
                co_move: ctx.co_move,
                eo_move: ctx.eo_move,
                slice_move: ctx.slice_move,
                cp_move: ctx.cp_move,
                max_fix_per_move: ctx.max_fix_per_move,
                has_limits: ctx.has_limits,
                limit_usages: ctx.limit_usages,
                limit_maxes: ctx.limit_maxes,
                limits_by_move: ctx.limits_by_move,
                path: ctx.path,
                solutions_found: ctx.solutions_found,
                nodes_explored: ctx.nodes_explored,
                shortest_length: ctx.shortest_length,
                cancelled: ctx.cancelled,
                aborted: ctx.aborted,
                output_mode: ctx.output_mode,
                top_n: ctx.top_n,
                transposition: ctx.transposition,
                cb: ctx.cb,
            },
            then_element_idx,
            moves_used,
            prev_axis,
            prev_base,
            mismatch,
        );
        if *ctx.aborted || *ctx.cancelled {
            return;
        }
    }
    if max_len == 0 {
        return;
    }
    rec_set(
        allowed,
        min_len,
        max_len,
        moves_used,
        0,
        prev_axis,
        prev_base,
        then_element_idx,
        &mut ctx,
    );
}

#[allow(clippy::too_many_arguments)]
fn rec_set(
    allowed: &[usize],
    min_len: usize,
    max_len: usize,
    moves_used: usize,
    placed: usize,
    axis: i32,
    base: i32,
    then_element_idx: usize,
    ctx: &mut EnumCtx<'_>,
) {
    if *ctx.cancelled || *ctx.aborted {
        return;
    }
    for &m in allowed {
        let b = move_base(m) as i32;
        let ax = AXIS_OF_BASE[b as usize] as i32;
        if ax == axis && b == base {
            continue;
        }
        if ax == axis && b < base {
            continue;
        }
        let ls_slot: Vec<usize>;
        let ls_opt: Option<&Vec<usize>>;
        if ctx.has_limits {
            ls_slot = ctx.limits_by_move[m].clone();
            let mut exceeded = false;
            for &li in &ls_slot {
                if ctx.limit_usages[li] >= ctx.limit_maxes[li] {
                    exceeded = true;
                    break;
                }
            }
            if exceeded {
                continue;
            }
            for &li in &ls_slot {
                ctx.limit_usages[li] += 1;
            }
            ls_opt = Some(&ls_slot);
        } else {
            ls_opt = None;
        }
        *ctx.nodes_explored += 1;
        ctx.path.push(m);
        let new_mism = apply_move_and_mismatch(
            moves_used + placed,
            m,
            ctx.move_perm_ref,
            ctx.state_stack,
            ctx.mismatch_stack,
            ctx.co_stack,
            ctx.eo_stack,
            ctx.slice_stack,
            ctx.cp_stack,
            ctx.k_co,
            ctx.k_eo,
            ctx.k_sl,
            ctx.use_coord,
            ctx.cp_co_some,
            ctx.co_move,
            ctx.eo_move,
            ctx.slice_move,
            ctx.cp_move,
            ctx.goal,
        );
        let new_placed = placed + 1;
        let h = lower_bound(
            moves_used + new_placed,
            new_mism,
            ctx.state_stack,
            ctx.goal,
            ctx.co_stack,
            ctx.eo_stack,
            ctx.slice_stack,
            ctx.cp_stack,
            ctx.k_co,
            ctx.k_eo,
            ctx.k_sl,
            ctx.co_table,
            ctx.eo_table,
            ctx.co_eo_table,
            ctx.cp_co_table,
            ctx.max_fix_per_move,
        );
        if h > (ctx.target_depth - (moves_used + new_placed)) as i32 {
            ctx.path.pop();
            if let Some(ls) = ls_opt {
                for &li in ls {
                    ctx.limit_usages[li] -= 1;
                }
            }
            if *ctx.aborted || *ctx.cancelled {
                return;
            }
            continue;
        }
        if new_placed >= min_len {
            dfs_node(
                DfsCtx {
                    target_depth: ctx.target_depth,
                    exact: ctx.exact,
                    compiled: ctx.compiled,
                    remaining_min: ctx.remaining_min,
                    suffix_invariants: ctx.suffix_invariants,
                    element_has_invariants: ctx.element_has_invariants,
                    goal: ctx.goal,
                    move_perm_ref: ctx.move_perm_ref,
                    state_stack: ctx.state_stack,
                    mismatch_stack: ctx.mismatch_stack,
                    co_stack: ctx.co_stack,
                    eo_stack: ctx.eo_stack,
                    slice_stack: ctx.slice_stack,
                    cp_stack: ctx.cp_stack,
                    k_co: ctx.k_co,
                    k_eo: ctx.k_eo,
                    k_sl: ctx.k_sl,
                    use_coord: ctx.use_coord,
                    co_table: ctx.co_table,
                    eo_table: ctx.eo_table,
                    co_eo_table: ctx.co_eo_table,
                    cp_co_table: ctx.cp_co_table,
                    cp_co_some: ctx.cp_co_some,
                    co_move: ctx.co_move,
                    eo_move: ctx.eo_move,
                    slice_move: ctx.slice_move,
                    cp_move: ctx.cp_move,
                    max_fix_per_move: ctx.max_fix_per_move,
                    has_limits: ctx.has_limits,
                    limit_usages: ctx.limit_usages,
                    limit_maxes: ctx.limit_maxes,
                    limits_by_move: ctx.limits_by_move,
                    path: ctx.path,
                    solutions_found: ctx.solutions_found,
                    nodes_explored: ctx.nodes_explored,
                    shortest_length: ctx.shortest_length,
                    cancelled: ctx.cancelled,
                    aborted: ctx.aborted,
                    output_mode: ctx.output_mode,
                    top_n: ctx.top_n,
                    transposition: ctx.transposition,
                    cb: ctx.cb,
                },
                then_element_idx,
                moves_used + new_placed,
                ax,
                b,
                new_mism,
            );
            if *ctx.aborted || *ctx.cancelled {
                ctx.path.pop();
                if let Some(ls) = ls_opt {
                    for &li in ls {
                        ctx.limit_usages[li] -= 1;
                    }
                }
                return;
            }
        }
        if new_placed < max_len {
            rec_set(
                allowed,
                min_len,
                max_len,
                moves_used,
                new_placed,
                ax,
                b,
                then_element_idx,
                ctx,
            );
        }
        ctx.path.pop();
        if let Some(ls) = ls_opt {
            for &li in ls {
                ctx.limit_usages[li] -= 1;
            }
        }
        if *ctx.aborted || *ctx.cancelled {
            return;
        }
    }
}

// ============================================================
// (Old `can_use_kociemba_fast_path` / `try_kociemba_fast_path` were
// removed per INCIDENTS.md INC-2026-05-18-01. The hybrid routing now
// lives in `search/routing.rs` and uses min2phase as an upper-bound
// oracle rather than an emission source. See plan v3 §「ハイブリッド
// routing 設計 (改訂版 — 最短性絶対保証)」.)
// ============================================================
