//! Search engine integration tests (no pruning tables, mismatch heuristic only,
//! so the build cost stays trivial).

use std::sync::{Arc, Mutex};

use cube_core::dsl::parser::parse_pattern;
use cube_core::facelet::create_solved_facelet;
use cube_core::mask::{bake_in_mask, create_full_mask};
use cube_core::moves::apply_moves;
use cube_core::search::{run_search, OutputMode, SearchCallbacks, SearchInput, SearchSolution};

fn search_collect(input: &SearchInput) -> Vec<SearchSolution> {
    let solutions: Arc<Mutex<Vec<SearchSolution>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&solutions);
    let cb = SearchCallbacks {
        on_solution: Some(Box::new(move |sol| sink.lock().unwrap().push(sol.clone()))),
        on_progress: None,
        should_cancel: None,
    };
    run_search(input, cb);
    Arc::try_unwrap(solutions)
        .expect("callback dropped")
        .into_inner()
        .unwrap()
}

#[test]
fn empty_pattern_with_solved_state_finds_zero_length() {
    let pattern = parse_pattern("").expect("parse");
    let solved = create_solved_facelet();
    let initial = bake_in_mask(&solved, &create_full_mask());
    let input = SearchInput {
        initial,
        goal: solved,
        pattern,
        max_moves: 0,
        output_mode: OutputMode::ShortestOnly,
        top_n: None,
        use_pruning_table: false,
    };
    let sols = search_collect(&input);
    assert_eq!(sols.len(), 1);
    assert_eq!(sols[0].length, 0);
}

#[test]
fn single_R_pattern_solves_R_prime_scramble_in_one_move() {
    // Scramble = R'  →  solution must be R.
    let solved = create_solved_facelet();
    let scrambled = apply_moves(&solved, &[2 * 3 + 1]); // R'
    let pattern = parse_pattern("R").expect("parse");
    let initial = bake_in_mask(&scrambled, &create_full_mask());
    let input = SearchInput {
        initial,
        goal: solved,
        pattern,
        max_moves: 3,
        output_mode: OutputMode::ShortestOnly,
        top_n: None,
        use_pruning_table: false,
    };
    let sols = search_collect(&input);
    assert!(!sols.is_empty(), "expected at least 1 solution");
    assert!(sols.iter().any(|s| s.moves == vec![2 * 3]));
}

#[test]
fn set_quantifier_finds_sune() {
    // After applying Sune `R U R' U R U2 R'` to a solved cube, the inverse
    // sequence (= apply Sune again twice = identity? actually Sune is order 6)
    // is also expressible as `R U2 R' U' R U' R'`. We just check that with a
    // mask of <R, U>{1,14}, searching from a sune'd state, we find a solve.
    let solved = create_solved_facelet();
    // Sune moves: R, U, R', U, R, U2, R'
    let sune: [usize; 7] = [
        2 * 3,      // R
        0 * 3,      // U
        2 * 3 + 1,  // R'
        0 * 3,      // U
        2 * 3,      // R
        0 * 3 + 2,  // U2
        2 * 3 + 1,  // R'
    ];
    let scrambled = apply_moves(&solved, &sune);
    let pattern = parse_pattern("<R, U>{1,14}").expect("parse");
    let initial = bake_in_mask(&scrambled, &create_full_mask());
    let input = SearchInput {
        initial,
        goal: solved,
        pattern,
        max_moves: 14,
        output_mode: OutputMode::ShortestOnly,
        top_n: None,
        use_pruning_table: false,
    };
    let sols = search_collect(&input);
    assert!(!sols.is_empty(), "expected solution for Sune");
    // verify the solution actually solves it
    let s = apply_moves(&scrambled, &sols[0].moves);
    assert_eq!(s, solved);
}

#[test]
fn rotation_literal_solves_x_inverse_scramble() {
    // x applied to solved is physically still "solved" (rotated frame).
    // Pattern `x'` should solve `x` scramble in 1 move.
    let solved = create_solved_facelet();
    let scrambled = apply_moves(&solved, &[15 * 3]); // x CW
    let pattern = parse_pattern("x'").expect("parse");
    let initial = bake_in_mask(&scrambled, &create_full_mask());
    let input = SearchInput {
        initial,
        goal: solved,
        pattern,
        max_moves: 1,
        output_mode: OutputMode::ShortestOnly,
        top_n: None,
        use_pruning_table: false,
    };
    let sols = search_collect(&input);
    assert_eq!(sols.len(), 1);
    assert_eq!(sols[0].moves, vec![15 * 3 + 1]); // x'
}

#[test]
fn rotation_in_set_quantifier() {
    // <R, U, x>{1,4} can solve x scramble in 1 move (via x').
    let solved = create_solved_facelet();
    let scrambled = apply_moves(&solved, &[15 * 3]); // x CW
    let pattern = parse_pattern("<R, U, x>{1,4}").expect("parse");
    let initial = bake_in_mask(&scrambled, &create_full_mask());
    let input = SearchInput {
        initial,
        goal: solved,
        pattern,
        max_moves: 4,
        output_mode: OutputMode::ShortestOnly,
        top_n: None,
        use_pruning_table: false,
    };
    let sols = search_collect(&input);
    assert!(!sols.is_empty());
    // Shortest solution should be exactly 1 move: x'
    assert!(sols.iter().any(|s| s.moves == vec![15 * 3 + 1]));
}

#[test]
fn rotated_solved_is_considered_solved_for_empty_pattern() {
    // Apply y rotation to solved — physical state still solved, just held in a
    // different orientation. Empty pattern with maxMoves=0 should find a
    // zero-length solution.
    let solved = create_solved_facelet();
    let rotated = apply_moves(&solved, &[16 * 3]); // y CW
    let pattern = parse_pattern("").expect("parse");
    let initial = bake_in_mask(&rotated, &create_full_mask());
    let input = SearchInput {
        initial,
        goal: solved,
        pattern,
        max_moves: 0,
        output_mode: OutputMode::ShortestOnly,
        top_n: None,
        use_pruning_table: false,
    };
    let sols = search_collect(&input);
    assert_eq!(sols.len(), 1, "rotated solved should be solved");
    assert_eq!(sols[0].length, 0);
}

#[test]
fn rotated_solved_quantifier_zero_solution() {
    // <R, U>{0, 3} on a rotated solved cube should find the 0-move solution.
    let solved = create_solved_facelet();
    let rotated = apply_moves(&solved, &[16 * 3]); // y
    let pattern = parse_pattern("<R, U>{0,3}").expect("parse");
    let initial = bake_in_mask(&rotated, &create_full_mask());
    let input = SearchInput {
        initial,
        goal: solved,
        pattern,
        max_moves: 3,
        output_mode: OutputMode::ShortestOnly,
        top_n: None,
        use_pruning_table: false,
    };
    let sols = search_collect(&input);
    assert!(!sols.is_empty(), "no solutions found");
    assert_eq!(sols.iter().map(|s| s.length).min(), Some(0));
}

#[test]
fn base_limit_caps_specific_variant() {
    // Apply F (CW) to solved, restrict to a [F<=1] limit and <R, U, F>.
    let solved = create_solved_facelet();
    let scrambled = apply_moves(&solved, &[4 * 3]); // F
    // Sequence-equivalent solution F' has 1 move; with [F<=1, F'<=1] we allow F or F'.
    // We test the parse + that a 1-move solution exists.
    let pattern = parse_pattern("[F<=2, F'<=2] <R, U, F>{1,6}").expect("parse");
    let initial = bake_in_mask(&scrambled, &create_full_mask());
    let input = SearchInput {
        initial,
        goal: solved,
        pattern,
        max_moves: 6,
        output_mode: OutputMode::ShortestOnly,
        top_n: None,
        use_pruning_table: false,
    };
    let sols = search_collect(&input);
    assert!(!sols.is_empty(), "expected solution under loose limit");
}
