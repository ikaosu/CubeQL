//! Coord encoding sanity — solved state encodes to (0, 0), CANONICAL_SLICE, CP=0.

use cube_core::coord::{
    apply_move_to_coord, decode_co, decode_corner_perm, decode_eo, decode_slice, encode_co,
    encode_coord, encode_corner_ori, encode_corner_perm, encode_corner_perm_from_state, encode_eo,
    encode_edge_ori, encode_slice, get_move_tables, CANONICAL_SLICE, NUM_CO_COORDS, NUM_EO_COORDS,
    NUM_SLICE_COORDS,
};
use cube_core::facelet::create_solved_facelet;
use cube_core::moves::{apply_moves, MOVE_COUNT};

#[test]
fn solved_state_encodes_to_zero_coords() {
    let solved = create_solved_facelet();
    assert_eq!(encode_corner_ori(&solved), 0);
    assert_eq!(encode_edge_ori(&solved), 0);
    assert_eq!(encode_slice(&solved), CANONICAL_SLICE as i32);
    assert_eq!(encode_coord(&solved), 0);
    assert_eq!(encode_corner_perm_from_state(&solved), 0);
}

#[test]
fn co_eo_encode_decode_roundtrip() {
    for &c in &[0u32, 1, 100, 1000, 2186] {
        let dec = decode_co(c);
        let re = encode_co(&dec);
        assert_eq!(re, c, "CO roundtrip failed for {}", c);
    }
    for &e in &[0u32, 1, 100, 1000, 2047] {
        let dec = decode_eo(e);
        let re = encode_eo(&dec);
        assert_eq!(re, e, "EO roundtrip failed for {}", e);
    }
}

#[test]
fn corner_perm_encode_decode_roundtrip() {
    for &c in &[0u32, 1, 100, 1000, 10000, 40319] {
        let dec = decode_corner_perm(c);
        let re = encode_corner_perm(&dec);
        assert_eq!(re, c, "CP roundtrip failed for {}", c);
    }
}

#[test]
fn slice_decode_canonical() {
    let occ = decode_slice(CANONICAL_SLICE);
    // canonical = positions 8..11 occupied
    for p in 0..12 {
        assert_eq!(occ[p], p >= 8, "slot {} occupancy expected {}", p, p >= 8);
    }
}

#[test]
fn move_table_co_updates_match_facelet_update() {
    let solved = create_solved_facelet();
    let tables = get_move_tables();
    for m in 0..MOVE_COUNT {
        let after = apply_moves(&solved, &[m]);
        let co_facelet = encode_corner_ori(&after);
        let co_via_table = tables.co[m * NUM_CO_COORDS + 0];
        assert_eq!(
            co_facelet as u32, co_via_table,
            "CO mismatch for move {}",
            m
        );
        let eo_facelet = encode_edge_ori(&after);
        let eo_via_table = tables.eo[m * NUM_EO_COORDS + 0];
        assert_eq!(eo_facelet as u32, eo_via_table, "EO mismatch for move {}", m);
        let sl_facelet = encode_slice(&after);
        let sl_via_table = tables.slice[m * NUM_SLICE_COORDS + (CANONICAL_SLICE as usize)];
        assert_eq!(sl_facelet as u32, sl_via_table, "slice mismatch for move {}", m);
    }
}

#[test]
fn apply_move_to_coord_matches_facelet_path() {
    let solved = create_solved_facelet();
    let mut state = solved;
    let mut coord = encode_coord(&state);
    // A non-trivial face-move sequence.
    let scramble: [usize; 5] = [2 * 3, 0 * 3, 2 * 3 + 1, 0 * 3 + 1, 4 * 3 + 2];
    for &m in &scramble {
        state = apply_moves(&state, &[m]);
        coord = apply_move_to_coord(coord, m);
    }
    let expected = encode_coord(&state);
    assert_eq!(coord, expected);
}
