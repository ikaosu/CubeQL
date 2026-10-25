//! Hybrid `run_search` routing (plan v3, INC-2026-05-18-01 fix).
//!
//! ### Design intent (do not weaken without `/bug` rollback)
//!
//! The hybrid is structured so that **the shortest solution is never
//! sacrificed**:
//!
//! 1. `min2phase` is run only as an **upper-bound oracle**. Its candidate is
//!    NEVER emitted unless we can prove it is optimal (PDB lower bound match
//!    OR IDA* exhaustion below `candidate.length` finds nothing).
//! 2. `IDA*+PDB iterative deepening` (the existing engine, `engine::run_ida_star`)
//!    is the only emission path. By construction, the first depth `d` at which
//!    IDA* finds a solution is the shortest under the user's constraints,
//!    because depths `0..d-1` were fully exhausted first.
//! 3. min2phase also short-circuits "no solution exists" for full-HTM
//!    unrestricted queries (e.g. scramble + maxMoves=11 where no 11-move
//!    solution exists). Without this, IDA*+PDB would exhaustively scan to
//!    depth 11 and hang.
//!
//! This module is the externally callable `run_search` entry. `engine::run_search`
//! is a thin re-export pointer.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use crate::coord::{
    encode_corner_ori, encode_corner_perm_from_state, encode_edge_ori, encode_slice,
    NUM_CO_COORDS, NUM_EO_COORDS, NUM_SLICE_COORDS,
};
use crate::dsl::pattern::{BaseLimit, Pattern, PatternElement};
use crate::facelet::{create_solved_facelet, Facelet, FACELET_COUNT};
use crate::mask::{is_goal, WILDCARD};
use crate::moves::{apply_moves, move_base, move_variant};
use crate::pruning::get_pruning_tables;
use crate::search::engine;
use crate::search::kociemba::try_solve_full_state;
use crate::search::{OutputMode, SearchCallbacks, SearchInput, SearchSolution, SearchStats};

// ----------------------- Public entry -----------------------

/// The public `run_search` entry, called by `engine::run_search` (which just
/// delegates here). See module docstring for design.
pub fn run_search_hybrid(input: &SearchInput, cb: SearchCallbacks) -> SearchStats {
    let start = Instant::now();

    let full_htm = is_full_HTM_unrestricted(input);

    // 0. Quick PDB lower-bound short-circuit (full-HTM unrestricted only).
    //    Admissible LB; if LB > max_moves, no solution can exist. Returns 0
    //    in ~ns and saves a doomed min2phase / IDA* run.
    if full_htm {
        let mut phys = create_solved_facelet();
        for i in 0..FACELET_COUNT {
            phys[i] = input.initial[i];
        }
        let lb = quick_pdb_lower_bound(&phys);
        if lb > input.max_moves {
            return SearchStats {
                total_solutions: 0,
                time_ms: start.elapsed().as_millis(),
                nodes_explored: 0,
                shortest_length: None,
            };
        }
    }

    // 1. min2phase oracle is only useful when:
    //    (a) the candidate length is likely close to input.max_moves so we
    //        get a tight upper bound for IDA*; and
    //    (b) min2phase actually finds something.
    //
    //    For **full-HTM unrestricted** queries with small `max_moves`
    //    relative to the cube's optimal length, min2phase exhausts all
    //    `length1` decompositions (0..21) before concluding "Error 8" —
    //    that can take many seconds. IDA*+PDB iterative deepening, in
    //    contrast, prunes effectively at small depths using the PDB tables
    //    and proves "no solution" in milliseconds. So we **skip min2phase
    //    when the query is full-HTM unrestricted with a small max_moves**
    //    and let IDA*+PDB handle it directly.
    //
    //    Threshold: 18 is a typical lower bound for random scramble optimal
    //    HTM length. Below that, min2phase is more likely to time out than
    //    save time.
    //
    //    The candidate is consumed ONLY by `ShortestOnly` (as an upper-bound
    //    oracle); `EnumerateUpToN` / `TopN` discard it (see dispatch below),
    //    so running min2phase for those modes is pure waste — and worse, when
    //    `max_moves` sits just below `phys`'s face-turn optimal, min2phase
    //    exhaustively proves "no face solution <= max_moves" and hangs
    //    ("starting...") before IDA* — which finds the real moveset solutions
    //    — ever runs. min2phase also produces an UNRESTRICTED face-turn solve,
    //    so for a restricted moveset / BaseLimits query its candidate is
    //    always Inapplicable. Gate on all three so the oracle only fires where
    //    it can actually help. (Bug: `[F2<=0,S2<=0] <U,R,F,Rw,M,S>` All<=N
    //    hung at maxMoves=13 but "found" 11-move solutions at 14.)
    let run_oracle = matches!(input.output_mode, OutputMode::ShortestOnly)
        && input.max_moves >= 18
        && is_min2phase_oracle_applicable(input);
    let candidate: Option<Vec<usize>> = if !run_oracle {
        None
    } else {
        match try_min2phase_oracle(input) {
            OracleResult::ProvedNone => {
                return SearchStats {
                    total_solutions: 0,
                    time_ms: start.elapsed().as_millis(),
                    nodes_explored: 0,
                    shortest_length: None,
                };
            }
            OracleResult::Candidate(sol) => Some(sol),
            OracleResult::Inapplicable => None,
        }
    };

    // 2. Dispatch by output mode
    match input.output_mode {
        OutputMode::ShortestOnly => run_shortest_only(input, cb, candidate, start),
        OutputMode::EnumerateUpToN | OutputMode::TopN => {
            // For Enumerate / TopN we just hand off to IDA*+PDB iterative
            // deepening, which naturally emits in depth order. min2phase's
            // candidate isn't useful as a single-solution upper bound here
            // because we want all/many solutions; IDA* would emit it
            // organically when it reaches that depth.
            engine::run_ida_star(input, cb)
        }
    }
}

fn run_shortest_only(
    input: &SearchInput,
    mut cb: SearchCallbacks,
    candidate: Option<Vec<usize>>,
    start: Instant,
) -> SearchStats {
    let Some(cand) = candidate else {
        // No candidate — just run IDA* with the user's max_moves
        return engine::run_ida_star(input, cb);
    };

    // Have a candidate of length L. We want to know the **shortest** valid
    // solution. Two paths:
    //   (a) IDA*+PDB iterative deepening for depths 0..L-1 finds something
    //       shorter → that's the shortest, emit it
    //   (b) IDA* exhausts 0..L-1 with no solution → candidate is the shortest
    //
    // To avoid emitting the candidate prematurely, we run IDA* with
    // max_moves=L-1, intercept its emissions to count, and only emit the
    // candidate ourselves if IDA* returned no solution.
    if cand.is_empty() {
        // Candidate is the zero-move solution. Initial state already matches
        // goal. Emit and return.
        emit_solution(&mut cb, &cand);
        return SearchStats {
            total_solutions: 1,
            time_ms: start.elapsed().as_millis(),
            nodes_explored: 0,
            shortest_length: Some(0),
        };
    }

    // Wrap on_solution so we can count IDA* emissions
    let user_on_sol = cb.on_solution.take();
    let user_on_sol_shared: Arc<Mutex<Option<Box<dyn FnMut(&SearchSolution) + Send + 'static>>>> =
        Arc::new(Mutex::new(user_on_sol));
    let user_on_sol_for_ida = Arc::clone(&user_on_sol_shared);
    let wrapped_on_sol = Box::new(move |sol: &SearchSolution| {
        let mut g = user_on_sol_for_ida.lock().unwrap();
        if let Some(f) = g.as_mut() {
            f(sol);
        }
    });

    let wrapped_cb = SearchCallbacks {
        on_solution: Some(wrapped_on_sol),
        on_progress: cb.on_progress,
        should_cancel: cb.should_cancel,
    };

    let mut sub_input = input.clone();
    sub_input.max_moves = cand.len() - 1;

    let stats = engine::run_ida_star(&sub_input, wrapped_cb);

    if stats.total_solutions > 0 {
        // IDA* found a shorter solution, already emitted via wrapped_cb
        return stats;
    }

    // IDA* found nothing in 0..L-1 → candidate IS the shortest. Emit candidate.
    {
        let mut g = user_on_sol_shared.lock().unwrap();
        if let Some(f) = g.as_mut() {
            let search_sol = SearchSolution {
                moves: cand.clone(),
                length: cand.len(),
            };
            f(&search_sol);
        }
    }

    SearchStats {
        total_solutions: 1,
        time_ms: start.elapsed().as_millis(),
        nodes_explored: stats.nodes_explored,
        shortest_length: Some(cand.len()),
    }
}

fn emit_solution(cb: &mut SearchCallbacks, moves: &[usize]) {
    if let Some(on_sol) = cb.on_solution.as_deref_mut() {
        let sol = SearchSolution {
            moves: moves.to_vec(),
            length: moves.len(),
        };
        on_sol(&sol);
    }
}

// ----------------------- min2phase oracle -----------------------

enum OracleResult {
    Candidate(Vec<usize>),
    /// `min2phase` returned None AND the query is full-HTM unrestricted, so
    /// **no solution exists** at the given depth. Routing returns 0 immediately.
    ProvedNone,
    /// min2phase doesn't apply, or returned a result that doesn't satisfy
    /// the user's constraints. Routing falls through to IDA*+PDB.
    Inapplicable,
}

fn try_min2phase_oracle(input: &SearchInput) -> OracleResult {
    // min2phase requires goal == solved
    if input.goal != create_solved_facelet() {
        return OracleResult::Inapplicable;
    }

    // Build physical facelet: replace WILDCARDs with solved color so min2phase
    // has a concrete facelet. A full-solve from such a substituted state ALSO
    // satisfies any mask, because mask just says "don't care about these
    // positions" — fully solved trivially matches.
    let mut phys = create_solved_facelet();
    for i in 0..FACELET_COUNT {
        let v = input.initial[i];
        if v != WILDCARD {
            phys[i] = v;
        }
    }

    if input.max_moves == 0 {
        return OracleResult::Inapplicable;
    }
    let max_depth = input.max_moves.min(25) as u8;

    let sol = try_solve_full_state(&phys, max_depth);

    match sol {
        Some(moves) => {
            if verify_solution_against_request(&moves, input) {
                OracleResult::Candidate(moves)
            } else {
                // min2phase returned a solution but it doesn't fit the pattern /
                // BaseLimits / mask constraints. Can't use it; can't claim
                // "no solution exists" either (a constraint-satisfying
                // shorter solution might exist via IDA*).
                OracleResult::Inapplicable
            }
        }
        None => {
            if is_full_HTM_unrestricted(input) {
                OracleResult::ProvedNone
            } else {
                OracleResult::Inapplicable
            }
        }
    }
}

/// Returns true when the search query is "full HTM, no constraints, no mask,
/// goal=solved, max_moves within min2phase cap". In this case `min2phase`
/// returning None is a **proof** that no solution exists at the given depth.
fn is_full_HTM_unrestricted(input: &SearchInput) -> bool {
    // No wildcards (full mask)
    if input.initial.iter().any(|&v| v == WILDCARD) {
        return false;
    }
    // Goal is solved
    if input.goal != create_solved_facelet() {
        return false;
    }
    // No BaseLimits
    if let Some(bl) = &input.pattern.base_limits {
        if !bl.is_empty() {
            return false;
        }
    }
    // Pattern is one MoveSet with all 6 face bases and default quantifier
    if input.pattern.elements.len() != 1 {
        return false;
    }
    let PatternElement::Set {
        allowed_bases,
        min,
        max,
        ..
    } = &input.pattern.elements[0]
    else {
        return false;
    };
    if *min != 0 || max.is_some() {
        return false;
    }
    let mut bases: Vec<usize> = allowed_bases.clone();
    bases.sort();
    bases.dedup();
    if bases != vec![0, 1, 2, 3, 4, 5] {
        return false;
    }
    // max_moves within min2phase internal cap
    if input.max_moves > 25 {
        return false;
    }
    true
}

/// Returns true when min2phase's UNRESTRICTED face-turn full solve could
/// plausibly satisfy the request, i.e. when running it as an oracle is worth
/// the cost. Unlike `is_full_HTM_unrestricted`, **wildcards (masks) are
/// allowed** — a full solve to solved trivially matches any mask, so masked
/// queries (OLL/PLL presets) still benefit. Rejects anything min2phase's
/// candidate could never satisfy: a non-solved goal, BaseLimits, or a moveset
/// that isn't the full six faces. For those, the candidate is guaranteed
/// Inapplicable, so calling min2phase is pure waste and risks the
/// "max_depth just below optimal" exhaustion hang.
fn is_min2phase_oracle_applicable(input: &SearchInput) -> bool {
    if input.goal != create_solved_facelet() {
        return false;
    }
    if let Some(bl) = &input.pattern.base_limits {
        if !bl.is_empty() {
            return false;
        }
    }
    if input.pattern.elements.len() != 1 {
        return false;
    }
    let PatternElement::Set {
        allowed_bases,
        min,
        max,
        ..
    } = &input.pattern.elements[0]
    else {
        return false;
    };
    if *min != 0 || max.is_some() {
        return false;
    }
    let mut bases: Vec<usize> = allowed_bases.clone();
    bases.sort();
    bases.dedup();
    bases == vec![0, 1, 2, 3, 4, 5]
}

// ----------------------- verify_solution -----------------------

/// Returns true iff `sol` satisfies the request's pattern decomposition,
/// BaseLimits, and mask-aware goal.
pub(crate) fn verify_solution_against_request(sol: &[usize], input: &SearchInput) -> bool {
    if !match_pattern(sol, &input.pattern) {
        return false;
    }
    if !check_base_limits(sol, &input.pattern.base_limits) {
        return false;
    }
    if !check_goal_mask_aware(sol, input) {
        return false;
    }
    true
}

fn check_goal_mask_aware(sol: &[usize], input: &SearchInput) -> bool {
    // Apply moves to the EffectiveFacelet (WILDCARDs travel with the
    // permutation) and compare to goal via mask-aware `is_goal`.
    let mut state: [i8; FACELET_COUNT] = [0i8; FACELET_COUNT];
    state.copy_from_slice(&input.initial);
    let after = apply_moves(&state, sol);
    is_goal(&after, &input.goal)
}

fn check_base_limits(sol: &[usize], limits: &Option<Vec<BaseLimit>>) -> bool {
    let Some(limits) = limits else {
        return true;
    };
    // Count variant occurrences AND base-wide occurrences
    let mut variant_counts: std::collections::HashMap<(usize, usize), usize> =
        std::collections::HashMap::new();
    let mut base_counts: std::collections::HashMap<usize, usize> =
        std::collections::HashMap::new();
    for &m in sol {
        let b = move_base(m);
        let v = move_variant(m);
        *variant_counts.entry((b, v)).or_insert(0) += 1;
        *base_counts.entry(b).or_insert(0) += 1;
    }
    for limit in limits {
        let count = match limit.variant {
            Some(v) => variant_counts.get(&(limit.base, v)).copied().unwrap_or(0),
            None => base_counts.get(&limit.base).copied().unwrap_or(0),
        };
        if count > limit.max {
            return false;
        }
    }
    true
}

/// Returns true iff `sol` can be decomposed into the pattern's element
/// sequence (literal → exact match; set → `min..=max` consecutive moves
/// with `move_base` ∈ `allowed_bases`).
fn match_pattern(sol: &[usize], pattern: &Pattern) -> bool {
    fn rec(sol: &[usize], elements: &[PatternElement], i: usize, el_idx: usize) -> bool {
        if el_idx == elements.len() {
            return i == sol.len();
        }
        if i > sol.len() {
            return false;
        }
        match &elements[el_idx] {
            PatternElement::Literal { move_idx, .. } => {
                if i < sol.len() && sol[i] == *move_idx {
                    rec(sol, elements, i + 1, el_idx + 1)
                } else {
                    false
                }
            }
            PatternElement::Set {
                allowed_bases,
                min,
                max,
                ..
            } => {
                let allowed_set: HashSet<usize> = allowed_bases.iter().copied().collect();
                let remaining = sol.len() - i;
                let max_v = max.unwrap_or(remaining);
                let upper = max_v.min(remaining);
                for k in *min..=upper {
                    // Check that all k consumed moves have base in allowed_set
                    let ok = (0..k).all(|j| allowed_set.contains(&move_base(sol[i + j])));
                    if !ok {
                        continue;
                    }
                    if rec(sol, elements, i + k, el_idx + 1) {
                        return true;
                    }
                }
                false
            }
        }
    }
    rec(sol, &pattern.elements, 0, 0)
}

// ----------------------- PDB quick lookup -----------------------

/// O(1) admissible HTM lower bound by joint PDB lookup. **Only valid for
/// fully-physical states (no wildcards)** — the caller must ensure the
/// facelet is concrete. Used at the head of `run_search_hybrid` to
/// short-circuit the "no solution at this depth" case in full-HTM
/// unrestricted queries without paying the cost of a doomed min2phase
/// call.
fn quick_pdb_lower_bound(state: &Facelet) -> usize {
    let tables = get_pruning_tables();
    let co = encode_corner_ori(state) as usize;
    let eo = encode_edge_ori(state) as usize;
    let slice = encode_slice(state) as usize;
    let cp = encode_corner_perm_from_state(state) as usize;

    let v_co_slice = tables.co_slice[co * NUM_SLICE_COORDS + slice] as usize;
    let v_eo_slice = tables.eo_slice[eo * NUM_SLICE_COORDS + slice] as usize;
    let v_co_eo = tables.co_eo[co * NUM_EO_COORDS + eo] as usize;
    let v_cp_co = tables.cp_co[cp * NUM_CO_COORDS + co] as usize;

    v_co_slice
        .max(v_eo_slice)
        .max(v_co_eo)
        .max(v_cp_co)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsl::parser::parse_pattern;
    use crate::facelet::create_solved_facelet;
    use crate::mask::create_full_mask;

    fn input_with(dsl: &str, max_moves: usize) -> SearchInput {
        let solved = create_solved_facelet();
        let mask = create_full_mask();
        let pattern = parse_pattern(dsl).unwrap();
        let initial = crate::mask::bake_in_mask(&solved, &mask);
        SearchInput {
            initial,
            goal: solved,
            pattern,
            max_moves,
            output_mode: OutputMode::ShortestOnly,
            top_n: None,
            use_pruning_table: true,
        }
    }

    #[test]
    fn full_htm_unrestricted_basic() {
        let input = input_with("<U,D,R,L,F,B>", 20);
        assert!(is_full_HTM_unrestricted(&input));
    }

    #[test]
    fn full_htm_rejects_quantifier_bounded() {
        let input = input_with("<U,D,R,L,F,B>{1,4}", 20);
        assert!(!is_full_HTM_unrestricted(&input));
    }

    #[test]
    fn full_htm_rejects_aliased_with_aufs() {
        let input = input_with("<U> <U,D,R,L,F,B> <U>", 20);
        assert!(!is_full_HTM_unrestricted(&input));
    }

    #[test]
    fn full_htm_rejects_partial_basis() {
        let input = input_with("<R,U>", 20);
        assert!(!is_full_HTM_unrestricted(&input));
    }

    #[test]
    fn verify_accepts_matching_pattern() {
        use crate::moves::parse_move_token;
        // pattern: <U><U,D,R,L,F,B><U>, sol: full HTM 7-move sequence (e.g. Sune)
        let input = input_with("<U> <U,D,R,L,F,B> <U>", 14);
        let sol: Vec<usize> = "R U R' U R U2 R'"
            .split_whitespace()
            .map(|t| parse_move_token(t).unwrap())
            .collect();
        // Apply Sune to solved -> non-goal state. So check_goal_mask_aware will be False.
        // But match_pattern should be True (sequence fits the pattern shape).
        assert!(match_pattern(&sol, &input.pattern));
    }

    #[test]
    fn quick_pdb_lb_reasonable_for_random_scramble() {
        use crate::moves::{apply_moves, parse_move_token};
        let scramble: Vec<usize> = "R F2 U2 R2 L B' R U L B' D B U' D2 F D2 R B R D F2 B' U B D"
            .split_whitespace()
            .map(|t| parse_move_token(t).unwrap())
            .collect();
        let solved = create_solved_facelet();
        let scrambled = apply_moves(&solved, &scramble);
        let lb = quick_pdb_lower_bound(&scrambled);
        eprintln!("quick_pdb_lb for test scramble = {}", lb);
        // For a random 25-move scramble, the joint PDB lower bound should be
        // at least 8 (typical 10-13).
        assert!(lb >= 7, "LB={} is suspiciously low", lb);
        // Sanity upper bound — these tables max at byte values, but unlikely
        // to be over 20 for realistic scrambles.
        assert!(lb < 50, "LB={} is suspiciously high (table likely wrong)", lb);
    }

    #[test]
    fn oracle_applicable_full_htm() {
        let input = input_with("<U,D,R,L,F,B>", 20);
        assert!(is_min2phase_oracle_applicable(&input));
    }

    #[test]
    fn oracle_not_applicable_restricted_moveset() {
        // Restricted moveset with wide/slice + BaseLimits: min2phase's
        // unrestricted face solve can never satisfy this, so the oracle must
        // not fire (it would hang at maxMoves just below phys optimal). This
        // is the reported `[F2<=0,S2<=0] <U,R,F,Rw,M,S>` bug shape.
        let input = input_with("[F2<=0, S2<=0] <U,R,F,Rw,M,S>", 13);
        assert!(!is_min2phase_oracle_applicable(&input));
    }

    #[test]
    fn oracle_applicable_masked_full_htm() {
        // Masked (wildcards) but full six faces — OLL/PLL preset shape. The
        // oracle SHOULD still apply (a full solve trivially matches the mask).
        use crate::mask::bake_in_mask;
        let solved = create_solved_facelet();
        let mut mask = crate::mask::create_full_mask();
        // Mask out a few non-U positions to simulate a preset.
        for i in 18..27 {
            mask[i] = 0;
        }
        let pattern = parse_pattern("<U,D,R,L,F,B>").unwrap();
        let initial = bake_in_mask(&solved, &mask);
        let input = SearchInput {
            initial,
            goal: solved,
            pattern,
            max_moves: 20,
            output_mode: OutputMode::ShortestOnly,
            top_n: None,
            use_pruning_table: true,
        };
        assert!(is_min2phase_oracle_applicable(&input));
    }

    #[test]
    fn verify_rejects_pattern_mismatch() {
        use crate::moves::parse_move_token;
        // pattern: <R,U>, sol contains F -> reject
        let input = input_with("<R,U>", 10);
        let sol: Vec<usize> = "R U F R'"
            .split_whitespace()
            .map(|t| parse_move_token(t).unwrap())
            .collect();
        assert!(!match_pattern(&sol, &input.pattern));
    }
}
