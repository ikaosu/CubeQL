//! `cube-cli cubie-bench` — microbench for the v4 cubie engine vs v3 facelet.

use std::process::ExitCode;
use std::time::Instant;

use cube_core::cubie::facelet_to_cubie;
use cube_core::cubie_pdb;
use cube_core::cubie_search::{build_goal_from_facelet_and_mask, search_cubie, search_cubie_parallel};
use cube_core::facelet::create_solved_facelet;
use cube_core::mask::create_full_mask;
use cube_core::moves::{apply_moves, parse_move_token};

pub fn cmd_cubie_bench(_args: &[String]) -> ExitCode {
    // Cases sized to be solvable with the current Phase v4-3 engine
    // (CO PDB only). Random full-scramble at depth 22 needs EO/slice
    // PDBs (Phase v4-6) to be tractable in single-thread.
    let cases: &[(&str, &str, usize)] = &[
        ("Sune (7 expected)", "R U R' U R U2 R'", 9),
        ("Anti-Sune (7)", "R' U' R U' R' U2 R", 9),
        ("F-sexy (6)", "F R U R' U' F'", 8),
        ("T-perm (~14)", "R U R' U' R' F R2 U' R' U' R U R' F'", 16),
        ("OLL Dot #1 (11)", "R U2 R2 F R F' U2 R' F R F'", 14),
    ];

    println!("Pre-building CO PDB...");
    let t0 = Instant::now();
    let _ = cubie_pdb::co_pdb();
    println!("CO PDB ready in {} ms", t0.elapsed().as_millis());

    let solved_f = create_solved_facelet();
    let mask = create_full_mask();
    let (gc, ge) = build_goal_from_facelet_and_mask(&solved_f, &mask);

    println!();
    println!(
        "{:<25}|{:>9}|{:>8}|{:>10}|{:>9}|{:>8}|{:>9}|{:>10}",
        "name", "max", "1T_sols", "1T_nodes", "1T_ms", "MT_sols", "MT_nodes", "MT_ms"
    );
    println!("{}", "-".repeat(100));
    for (name, scramble, depth) in cases {
        let moves: Vec<usize> = scramble
            .split_whitespace()
            .filter_map(parse_move_token)
            .collect();
        let scrambled_f = apply_moves(&solved_f, &moves);
        let initial = facelet_to_cubie(&scrambled_f);
        // Per-case deadline (~30s) for each engine variant
        let deadline_st = Instant::now() + std::time::Duration::from_secs(30);
        let mut cancel_st = move || Instant::now() >= deadline_st;
        let (st_sols, st_nodes, st_ms) = search_cubie(
            &initial, &gc, &ge, *depth, [false; 6], false, &mut cancel_st,
        );
        let deadline_mt = Instant::now() + std::time::Duration::from_secs(30);
        let cancel_mt = move || Instant::now() >= deadline_mt;
        let (mt_sols, mt_nodes, mt_ms) = search_cubie_parallel(
            &initial, &gc, &ge, *depth, [false; 6], false, &cancel_mt,
        );
        println!(
            "{:<25}|{:>9}|{:>8}|{:>10}|{:>9}|{:>8}|{:>9}|{:>10}",
            name, depth, st_sols.len(), st_nodes, st_ms, mt_sols.len(), mt_nodes, mt_ms
        );
    }
    ExitCode::from(0)
}
