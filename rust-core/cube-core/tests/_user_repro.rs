use std::sync::{Arc, Mutex};

use cube_core::dsl::parser::parse_pattern;
use cube_core::facelet::create_solved_facelet;
use cube_core::mask::{bake_in_mask, create_full_mask};
use cube_core::moves::{apply_moves, parse_move_token};
use cube_core::pruning::get_pruning_tables;
use cube_core::rotations::is_solved_rotation_tolerant;
use cube_core::search::{run_search, OutputMode, SearchCallbacks, SearchInput, SearchSolution};

fn parse(s: &str) -> Vec<usize> {
    s.split_whitespace().map(|t| parse_move_token(t).expect(t)).collect()
}

#[test]
fn user_scramble_and_solutions() {
    let solved = create_solved_facelet();
    // User wrote R2' (= R2 since 180° has no direction). Normalize before parsing.
    let scramble = parse("R' U' R D' U R2 U R' U R U' R U' R2 D");
    let scrambled = apply_moves(&solved, &scramble);
    eprintln!("scrambled rot-tol-solved: {}", is_solved_rotation_tolerant(&scrambled, &solved));

    let sol14 = parse("R2 Uw R' U R' U' R Uw' R2 y' R' U R U'");
    let sol15 = parse("R2 Uw R' U R' U' R Uw' R2 y' R' U R y U'");

    for k in 0..=sol15.len() {
        let s = apply_moves(&scrambled, &sol15[..k]);
        let tol = is_solved_rotation_tolerant(&s, &solved);
        eprintln!("scramble + sol15[..{}] rot-tol-solved={}", k, tol);
    }

    let after14 = apply_moves(&scrambled, &sol14);
    eprintln!("scramble + sol14 (user) rot-tol-solved={}", is_solved_rotation_tolerant(&after14, &solved));

    let after15 = apply_moves(&scrambled, &sol15);
    eprintln!("scramble + sol15 (engine) rot-tol-solved={}", is_solved_rotation_tolerant(&after15, &solved));
}

#[test]
fn user_engine_should_find_14_move_solution() {
    if std::env::var("GENCUBE_BUILD_PRUNING").ok().as_deref() != Some("1") {
        eprintln!("skipped (set GENCUBE_BUILD_PRUNING=1)");
        return;
    }
    let _ = get_pruning_tables();
    let solved = create_solved_facelet();
    let scramble = parse("R' U' R D' U R2 U R' U R U' R U' R2 D");
    let scrambled = apply_moves(&solved, &scramble);

    let pattern = parse_pattern("<R, U, Uw, y>").expect("parse");
    let initial = bake_in_mask(&scrambled, &create_full_mask());

    let input = SearchInput {
        initial,
        goal: solved,
        pattern,
        max_moves: 17,
        output_mode: OutputMode::ShortestOnly,
        top_n: None,
        use_pruning_table: true,
    };
    let sols: Arc<Mutex<Vec<SearchSolution>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&sols);
    let cb = SearchCallbacks {
        on_solution: Some(Box::new(move |s| sink.lock().unwrap().push(s.clone()))),
        on_progress: None,
        should_cancel: None,
    };
    let stats = run_search(&input, cb);
    let collected = Arc::try_unwrap(sols).unwrap().into_inner().unwrap();
    eprintln!("stats: total={} shortest={:?} time_ms={} nodes={}",
        stats.total_solutions, stats.shortest_length, stats.time_ms, stats.nodes_explored);
    for s in &collected {
        eprintln!("  len {}: moves {:?}", s.length, s.moves);
    }
    let min_len = collected.iter().map(|s| s.length).min().unwrap_or(usize::MAX);
    assert!(min_len <= 14, "shortest should be <= 14 (user's solution), got {}", min_len);
}
