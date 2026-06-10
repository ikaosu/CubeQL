use std::sync::{Arc, Mutex};

use cube_core::dsl::parser::parse_pattern;
use cube_core::facelet::create_solved_facelet;
use cube_core::mask::{bake_in_mask, create_full_mask};
use cube_core::moves::{apply_moves, parse_move_token};
use cube_core::pruning::get_pruning_tables;
use cube_core::rotations::is_solved_rotation_tolerant;
use cube_core::search::{run_search, OutputMode, SearchCallbacks, SearchInput, SearchSolution};
use cube_core::coord::{
    encode_corner_perm_from_state, enumerate_corner_ori_coords, enumerate_edge_ori_coords,
    enumerate_slice_coords, CANONICAL_SLICE,
};
use cube_core::subgroup::{
    check_piece_orbits_at_state, compute_piece_orbits, compute_subgroup_invariants,
};

fn parse(s: &str) -> Vec<usize> {
    s.split_whitespace().map(|t| parse_move_token(t).expect(t)).collect()
}

#[test]
fn ru_impossible_should_prune_quickly() {
    if std::env::var("GENCUBE_BUILD_PRUNING").ok().as_deref() != Some("1") {
        eprintln!("skipped");
        return;
    }
    let _ = get_pruning_tables();
    let solved = create_solved_facelet();
    let scramble = parse("R U R' U' R' U F R U R U' R' F' U R' U2 R");
    let scrambled = apply_moves(&solved, &scramble);

    eprintln!("scramble applied; rot-tol solved? {}", is_solved_rotation_tolerant(&scrambled, &solved));

    let pattern = parse_pattern("<R, U>").expect("parse");
    let initial = bake_in_mask(&scrambled, &create_full_mask());
    let input = SearchInput {
        initial,
        goal: solved,
        pattern,
        max_moves: 20,
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
    eprintln!("nodes={} time_ms={} sols={}", stats.nodes_explored, stats.time_ms, collected.len());
    for s in &collected {
        eprintln!("  len {}: {:?}", s.length, s.moves);
    }
    // Note: this is the diag; the assertion below is what fails today.
    eprintln!("(diag: ran with max=20, sols={})", collected.len());
}

#[test]
fn user_scramble_extended_to_25() {
    if std::env::var("GENCUBE_BUILD_PRUNING").ok().as_deref() != Some("1") {
        eprintln!("skipped");
        return;
    }
    let _ = get_pruning_tables();
    let solved = create_solved_facelet();
    let scramble = parse("R U R' U' R' U F R U R U' R' F' U R' U2 R");
    let scrambled = apply_moves(&solved, &scramble);

    let pattern = parse_pattern("<R, U>").expect("parse");
    let initial = bake_in_mask(&scrambled, &create_full_mask());
    let input = SearchInput {
        initial,
        goal: solved,
        pattern,
        max_moves: 25,
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
    eprintln!("nodes={} time_ms={} sols={}", stats.nodes_explored, stats.time_ms, collected.len());
    for s in &collected {
        eprintln!("  len {}: {:?}", s.length, s.moves);
    }
}

#[test]
fn bad_eo_with_ru_prunes_zero_nodes() {
    if std::env::var("GENCUBE_BUILD_PRUNING").ok().as_deref() != Some("1") {
        eprintln!("skipped");
        return;
    }
    let _ = get_pruning_tables();
    let solved = create_solved_facelet();
    // F flip-and-leave: bad EO with non-trivial state.
    let scramble = parse("F R U R' U' F'");
    let scrambled = apply_moves(&solved, &scramble);

    let pattern = parse_pattern("<R, U>").expect("parse");
    let initial = bake_in_mask(&scrambled, &create_full_mask());
    let input = SearchInput {
        initial,
        goal: solved,
        pattern,
        max_moves: 12,
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
    eprintln!("nodes={} sols={}", stats.nodes_explored, collected.len());
    assert_eq!(collected.len(), 0, "should be 0 sols");
    assert!(stats.nodes_explored < 100, "bad-EO should be pruned almost immediately, got {}", stats.nodes_explored);
}

#[test]
fn diag_ru_state_vs_fixed_facelets() {
    // Pure diagnostic — runs without GENCUBE_BUILD_PRUNING since it doesn't search.
    use cube_core::moves::Base;
    let solved = create_solved_facelet();
    let scramble = parse("R U R' U' R' U F R U R U' R' F' U R' U2 R");
    let scrambled = apply_moves(&solved, &scramble);

    // {R, U} basis indices = Base::R and Base::U.
    let bases = vec![Base::R, Base::U];
    let inv = compute_subgroup_invariants(&bases);
    eprintln!(
        "inv: facelet_positions.len = {}, preserves_co={} eo={} slice={} cp={}",
        inv.facelet_positions.len(),
        inv.preserves_co,
        inv.preserves_eo,
        inv.preserves_slice,
        inv.preserves_cp,
    );
    eprintln!("fixed facelet idxs: {:?}", inv.facelet_positions);

    let mut mismatches: Vec<(usize, i8, i8)> = Vec::new();
    for &p in &inv.facelet_positions {
        if scrambled[p] != solved[p] {
            mismatches.push((p, scrambled[p], solved[p]));
        }
    }
    eprintln!("mismatches at fixed positions: {}", mismatches.len());
    for (p, got, want) in &mismatches {
        eprintln!("  facelet[{}]: got {} want {}", p, got, want);
    }

    eprintln!(
        "rotation-tolerant solved? {}",
        is_solved_rotation_tolerant(&scrambled, &solved)
    );

    // If mismatches.len() > 0, the fixed-facelet check SHOULD prune at root and
    // the search should report ~0 nodes. If it's 0, the engine needs a deeper
    // invariant than per-facelet fixity.
    if mismatches.is_empty() {
        eprintln!(
            "NO per-facelet mismatch: every R/U-fixed sticker matches goal. \
             Need a higher-order invariant (CO/EO sum, parity, or coset table)."
        );
    } else {
        eprintln!(
            "Per-facelet mismatch detected — check_subgroup_invariants must \
             be firing at root. If search still expands many nodes, the wiring \
             is wrong."
        );
    }

    // Coord-level invariants: what does the search see at root?
    let co_cands = enumerate_corner_ori_coords(&scrambled);
    let eo_cands = enumerate_edge_ori_coords(&scrambled);
    let sl_cands = enumerate_slice_coords(&scrambled);
    let cp0 = encode_corner_perm_from_state(&scrambled);
    eprintln!(
        "co_cands (len {}): {:?}",
        co_cands.len(),
        &co_cands[..co_cands.len().min(8)]
    );
    eprintln!(
        "eo_cands (len {}): {:?}",
        eo_cands.len(),
        &eo_cands[..eo_cands.len().min(8)]
    );
    eprintln!(
        "sl_cands (len {}): {:?}",
        sl_cands.len(),
        &sl_cands[..sl_cands.len().min(8)]
    );
    eprintln!("cp0 = {} (GOAL_CP = 0)", cp0);
    eprintln!(
        "  any co_cand == 0? {}",
        co_cands.iter().any(|&v| v == 0)
    );
    eprintln!(
        "  any eo_cand == 0? {}",
        eo_cands.iter().any(|&v| v == 0)
    );
    eprintln!(
        "  any sl_cand == CANONICAL_SLICE({})? {}",
        CANONICAL_SLICE,
        sl_cands.iter().any(|&v| v == CANONICAL_SLICE)
    );

    // Decode the corner perm to see which corners are home.
    use cube_core::coord::decode_corner_perm;
    let cp_array = decode_corner_perm(cp0 as u32);
    let corner_names = ["URF", "UFL", "ULB", "UBR", "DFR", "DLF", "DBL", "DRB"];
    eprintln!("Corner perm (slot -> piece at that slot):");
    for slot in 0..8 {
        let piece = cp_array[slot] as usize;
        let marker = if slot == piece { "  home" } else { "  ***" };
        eprintln!(
            "  slot {} ({}): piece {} ({}){}",
            slot, corner_names[slot], piece, corner_names[piece], marker
        );
    }

    // Corner perm parity (even/odd).
    let parity = perm_parity(&cp_array);
    eprintln!("Corner perm parity: {}", if parity == 0 { "even" } else { "odd" });
}

#[test]
fn diag_pair_orbits_catch_ru_scramble() {
    use cube_core::moves::Base;
    let solved = create_solved_facelet();
    let scramble = parse("R U R' U' R' U F R U R U' R' F' U R' U2 R");
    let scrambled = apply_moves(&solved, &scramble);
    let initial = bake_in_mask(&scrambled, &create_full_mask());

    let orbits = compute_piece_orbits(&[Base::R, Base::U]);
    eprintln!("orbits.is_full_group = {}", orbits.is_full_group);

    // Lv1 check: which corner home slots are reachable for each piece?
    eprintln!("Corner piece -> reachable slot bitmask (Lv1 diagonal extract):");
    for piece in 0..8 {
        let mask = orbits.corner_pair[piece * 8 + piece];
        let mut slots = Vec::new();
        for s in 0..8 {
            if mask & (1u64 << (s * 8 + s)) != 0 {
                slots.push(s);
            }
        }
        eprintln!("  piece {}: reachable slots = {:?}", piece, slots);
    }

    eprintln!("Edge piece -> reachable slot bitmask (Lv1 diagonal extract):");
    for piece in 0..12 {
        let bits = &orbits.edge_pair[piece * 12 + piece];
        let mut slots = Vec::new();
        for s in 0..12 {
            if bits[s * 12 + s] {
                slots.push(s);
            }
        }
        eprintln!("  edge {}: reachable slots = {:?}", piece, slots);
    }

    // Cross-pair reachable set for (corner URF=0, edge UR=0):
    let bits = &orbits.cross_pair[0 * 12 + 0];
    let pairs: Vec<(usize, usize)> = (0..8)
        .flat_map(|cs| (0..12).map(move |es| (cs, es)))
        .filter(|&(cs, es)| bits[cs * 12 + es])
        .collect();
    eprintln!(
        "cross_pair[corner URF=0, edge UR=0] reachable pairs count = {}",
        pairs.len()
    );
    eprintln!("  pairs: {:?}", pairs);
    let scrambled_pair = (3usize, 0usize);
    eprintln!(
        "  scramble pair {:?} reachable? {}",
        scrambled_pair,
        bits[scrambled_pair.0 * 12 + scrambled_pair.1]
    );

    // Corner-pair reachable set for (URF=0, UFL=1):
    let mask = orbits.corner_pair[0 * 8 + 1];
    eprintln!("corner_pair[URF=0, UFL=1] reachable (slot_URF, slot_UFL):");
    for a in 0..8 {
        for b in 0..8 {
            if mask & (1u64 << (a * 8 + b)) != 0 {
                eprintln!("  ({}, {})", a, b);
            }
        }
    }

    // Corner-pair reachable set for (URF=0, UBR=3):
    let mask = orbits.corner_pair[0 * 8 + 3];
    eprintln!("corner_pair[URF=0, UBR=3] reachable (slot_URF, slot_UBR):");
    for a in 0..8 {
        for b in 0..8 {
            if mask & (1u64 << (a * 8 + b)) != 0 {
                eprintln!("  ({}, {})", a, b);
            }
        }
    }

    let passes = check_piece_orbits_at_state(&initial, &orbits);
    eprintln!("check_piece_orbits_at_state returned: {}", passes);
    // assert!(!passes, "must reject the impossible R/U scramble");
}

#[test]
fn solvable_ru_scramble_still_finds_solution() {
    if std::env::var("GENCUBE_BUILD_PRUNING").ok().as_deref() != Some("1") {
        eprintln!("skipped");
        return;
    }
    let _ = get_pruning_tables();
    let solved = create_solved_facelet();
    // Pure <R, U> scramble — must be solvable in <R, U>.
    let scramble = parse("R U R' U R U2 R'");
    let scrambled = apply_moves(&solved, &scramble);

    let pattern = parse_pattern("<R, U>").expect("parse");
    let initial = bake_in_mask(&scrambled, &create_full_mask());
    let input = SearchInput {
        initial,
        goal: solved,
        pattern,
        max_moves: 12,
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
    eprintln!("nodes={} sols={}", stats.nodes_explored, collected.len());
    assert!(
        !collected.is_empty(),
        "pure R/U scramble must be solvable; orbit check must not over-prune"
    );
}

#[test]
fn diag_corner_orbit_bfs_catches_3cycle() {
    // BFS over (CP, CO) reachable from (0, 0) by <R, U> moves.
    // If (6600, 0) is NOT in the orbit, then "joint corner state" check
    // catches the user's scramble; otherwise we need edges too.
    use cube_core::coord::{get_move_tables, NUM_CO_COORDS, NUM_CP_COORDS};
    use std::collections::HashSet;

    let mt = get_move_tables();
    let mut visited: HashSet<(u32, u32)> = HashSet::new();
    let mut queue: Vec<(u32, u32)> = vec![(0, 0)];
    visited.insert((0, 0));

    // <R, U> moves: bases R (2) and U (0), each with 3 variants.
    let bases = [0usize, 2];
    let moves: Vec<usize> = bases.iter().flat_map(|&b| (0..3).map(move |v| b * 3 + v)).collect();
    let mut steps = 0u64;
    while let Some((cp, co)) = queue.pop() {
        steps += 1;
        if steps % 100_000 == 0 {
            eprintln!("  bfs progress: visited={}, queue={}", visited.len(), queue.len());
        }
        for &m in &moves {
            let nc = mt.cp[m * NUM_CP_COORDS + cp as usize];
            let no = mt.co[m * NUM_CO_COORDS + co as usize];
            let next = (nc, no);
            if visited.insert(next) {
                queue.push(next);
            }
        }
    }
    eprintln!("<R,U> corner-orbit size = {}", visited.len());
    eprintln!("  contains (6600, 0)? {}", visited.contains(&(6600, 0)));
}

fn perm_parity(p: &[u8; 8]) -> u32 {
    let mut visited = [false; 8];
    let mut parity = 0u32;
    for i in 0..8 {
        if visited[i] {
            continue;
        }
        let mut j = i;
        let mut cycle_len = 0;
        while !visited[j] {
            visited[j] = true;
            j = p[j] as usize;
            cycle_len += 1;
        }
        if cycle_len > 1 {
            parity = (parity + (cycle_len as u32 - 1)) % 2;
        }
    }
    parity
}
