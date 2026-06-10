//! Rust ↔ TS performance bench parity.
//!
//! Mirrors `03_develop/tests/bench.spec.ts` so we can compare Rust and TS on
//! identical scrambles / DSL / mask / output mode. Gated behind the env var
//! `GENCUBE_BUILD_PRUNING=1` because pruning-table build may take ~3 minutes
//! on first run (subsequent runs hit the disk cache and are sub-second).
//!
//! Run with:
//!   $env:GENCUBE_BUILD_PRUNING = "1"
//!   cargo test --release -p cube-core --test perf_bench -- --nocapture --test-threads=1
//!
//! TS reference numbers (from TS vitest, 2026-05-15):
//!   OLL Sune <R, U> N=14 shortest:           5,031 nodes,    284 ms
//!   OLL Sune <R, U, F> N=12 shortest:      115,233 nodes,    152 ms
//!   hard      <R, U, F> N=10 shortest:   3,540,081 nodes,  6,274 ms
//!   V-perm    <R, U, F> N=18 shortest:   4,127,385 nodes, 10,843 ms

use std::time::Instant;
use std::sync::{Arc, Mutex};

use cube_core::dsl::parser::parse_pattern;
use cube_core::facelet::create_solved_facelet;
use cube_core::mask::bake_in_mask;
use cube_core::moves::{apply_moves, inverse_move, move_of, Base};
use cube_core::presets::mask_from_preset_id;
use cube_core::pruning::get_pruning_tables;
use cube_core::search::{
    run_search, OutputMode, SearchCallbacks, SearchInput, SearchSolution,
};

fn enabled() -> bool {
    std::env::var("GENCUBE_BUILD_PRUNING").ok().as_deref() == Some("1")
}

fn run_bench(
    label: &str,
    scrambled: [i8; 54],
    mask: [u8; 54],
    pattern_src: &str,
    max_moves: usize,
    output_mode: OutputMode,
    use_pruning_table: bool,
) -> (usize, u128) {
    let pattern = parse_pattern(pattern_src).expect("parse");
    let initial = bake_in_mask(&scrambled, &mask);
    let goal = create_solved_facelet();

    let input = SearchInput {
        initial,
        goal,
        pattern,
        max_moves,
        output_mode,
        top_n: None,
        use_pruning_table,
    };
    let sols: Arc<Mutex<Vec<SearchSolution>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&sols);
    let cb = SearchCallbacks {
        on_solution: Some(Box::new(move |s| sink.lock().unwrap().push(s.clone()))),
        on_progress: None,
        should_cancel: None,
    };
    let t0 = Instant::now();
    let stats = run_search(&input, cb);
    let ms = t0.elapsed().as_millis();
    let n = sols.lock().unwrap().len();
    println!(
        "  {}: {} sols, {} nodes, {} ms (shortest={:?})",
        label, n, stats.nodes_explored, ms, stats.shortest_length
    );
    (stats.nodes_explored, ms)
}

#[test]
fn perf_bench() {
    if !enabled() {
        println!("skipped (set GENCUBE_BUILD_PRUNING=1 to enable)");
        return;
    }
    // Build / load pruning tables once. Subsequent calls inside run_bench will
    // hit the OnceLock'd tables.
    let t0 = Instant::now();
    let _ = get_pruning_tables();
    println!("pruning tables ready in {} ms", t0.elapsed().as_millis());

    let solved = create_solved_facelet();
    let oll_mask = mask_from_preset_id("OLL").expect("OLL preset");

    // === Sune scramble (inverse of R U R' U R U2 R') ===
    let sune: [usize; 7] = [
        move_of(Base::R, 0), move_of(Base::U, 0), move_of(Base::R, 1),
        move_of(Base::U, 0), move_of(Base::R, 0), move_of(Base::U, 2),
        move_of(Base::R, 1),
    ];
    let scramble_sune: Vec<usize> = sune.iter().rev().map(|&m| inverse_move(m)).collect();
    let scrambled_sune = apply_moves(&solved, &scramble_sune);

    println!("--- OLL Sune <R, U> N=14 shortest (TS: 5,031 nodes / 284 ms) ---");
    run_bench(
        "rust no-prune",
        scrambled_sune,
        oll_mask,
        "<R, U>",
        14,
        OutputMode::ShortestOnly,
        false,
    );
    run_bench(
        "rust prune",
        scrambled_sune,
        oll_mask,
        "<R, U>",
        14,
        OutputMode::ShortestOnly,
        true,
    );

    println!("--- OLL Sune <R, U, F> N=12 shortest (TS prune OFF: 115,233 nodes / 152 ms) ---");
    run_bench(
        "rust no-prune",
        scrambled_sune,
        oll_mask,
        "<R, U, F>",
        12,
        OutputMode::ShortestOnly,
        false,
    );
    run_bench(
        "rust prune",
        scrambled_sune,
        oll_mask,
        "<R, U, F>",
        12,
        OutputMode::ShortestOnly,
        true,
    );

    // === hard scramble: R U R' F U R U' R' F' (mirrors TS bench) ===
    let hard_scramble: [usize; 9] = [
        move_of(Base::R, 0), move_of(Base::U, 0), move_of(Base::R, 1),
        move_of(Base::F, 0), move_of(Base::U, 0), move_of(Base::R, 0),
        move_of(Base::U, 1), move_of(Base::R, 1), move_of(Base::F, 1),
    ];
    let scrambled_hard = apply_moves(&solved, &hard_scramble);

    println!("--- hard <R, U, F> N=10 shortest (TS prune OFF: 3,540,081 nodes / 6,274 ms) ---");
    run_bench(
        "rust no-prune",
        scrambled_hard,
        oll_mask,
        "<R, U, F>",
        10,
        OutputMode::ShortestOnly,
        false,
    );
    run_bench(
        "rust prune",
        scrambled_hard,
        oll_mask,
        "<R, U, F>",
        10,
        OutputMode::ShortestOnly,
        true,
    );

    // === V-perm style deep <R, U, F> scramble (mirrors TS pruning-bench) ===
    // Apples-to-apples with TS: same 15-move scramble + full mask + N=18.
    let vperm_scramble: [usize; 15] = [
        move_of(Base::R, 0), move_of(Base::U, 0), move_of(Base::R, 0),
        move_of(Base::U, 0), move_of(Base::R, 0), move_of(Base::U, 0),
        move_of(Base::R, 0), move_of(Base::F, 0), move_of(Base::R, 0),
        move_of(Base::F, 0), move_of(Base::R, 0), move_of(Base::F, 0),
        move_of(Base::R, 0), move_of(Base::F, 0), move_of(Base::R, 0),
    ];
    let scrambled_vperm = apply_moves(&solved, &vperm_scramble);
    let full_mask: [u8; 54] = [1u8; 54];

    println!("--- V-perm style full-mask <R, U, F> N=18 shortest (TS prune ON: 4,127,385 nodes / 10,843 ms) ---");
    run_bench(
        "rust prune",
        scrambled_vperm,
        full_mask,
        "<R, U, F>",
        18,
        OutputMode::ShortestOnly,
        true,
    );

    // === Rotation pruning sanity (v2 cache must include rotation moves) ===
    println!("--- Sune <R, U, F, y> N=12 shortest (rotation in allowed set) ---");
    run_bench(
        "rust no-prune",
        scrambled_sune,
        oll_mask,
        "<R, U, F, y>",
        12,
        OutputMode::ShortestOnly,
        false,
    );
    run_bench(
        "rust prune",
        scrambled_sune,
        oll_mask,
        "<R, U, F, y>",
        12,
        OutputMode::ShortestOnly,
        true,
    );

    println!("--- hard <R, U, F, x, y, z> N=10 shortest (rotation in allowed set) ---");
    run_bench(
        "rust prune",
        scrambled_hard,
        oll_mask,
        "<R, U, F, x, y, z>",
        10,
        OutputMode::ShortestOnly,
        true,
    );
}
