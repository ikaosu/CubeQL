//! Partial coordinate encodings on cubie state.
//!
//! These coordinates compress the full cubie state into integer indices
//! that PDB tables can be keyed on. Each coord type focuses on one aspect
//! (corner orientation, edge orientation, slice position, …). For masked
//! states, an `unknown` bitmask records which slots are unconstrained;
//! BFS/PDB lookups treat those slots permissively.
//!
//! All encodings use canonical face-axis conventions (CO mod 3, EO mod 2),
//! matching `moves.rs::base_data()` and `cubie.rs::build_face_moves()`.

use crate::cubie::{
    CubieState, ORIENTATION_UNKNOWN, POSITION_UNKNOWN, NUM_CORNERS, NUM_EDGES,
};

// Sizes of the coord spaces (number of distinct values).

/// 3^7 = 2187. The 8th corner's orientation is determined by the others
/// (total CO must be 0 mod 3 for a valid cube), so encode only the first 7.
pub const NUM_CO_COORDS: usize = 2187;

/// 2^11 = 2048. The 12th edge's orientation is determined by the others.
pub const NUM_EO_COORDS: usize = 2048;

/// C(12, 4) = 495. Number of distinct placements of the 4 E-slice edges.
pub const NUM_SLICE_COORDS: usize = 495;

/// 8! = 40320 corner permutations.
pub const NUM_CP_COORDS: usize = 40320;

/// Compute the corner orientation coordinate from cubie state.
///
/// Returns `(coord, unknown_mask)`. `coord` is 0..NUM_CO_COORDS (or
/// arbitrary if any corner ori is unknown — caller must check the mask).
/// `unknown_mask` is a u8 with bit i set iff corner slot i has unknown ori.
pub fn corner_ori_coord(state: &CubieState) -> (u32, u8) {
    let mut coord: u32 = 0;
    let mut unknown: u8 = 0;
    // Encode the first 7 corner orientations in base 3 (slot 7 is implicit)
    for i in 0..(NUM_CORNERS - 1) {
        let ori = state.corners[i].ori;
        if ori == ORIENTATION_UNKNOWN {
            unknown |= 1 << i;
            continue; // skip but keep encoding consistent
        }
        coord = coord * 3 + (ori as u32);
    }
    // Check the 8th corner's ori too for unknown bit
    if state.corners[NUM_CORNERS - 1].ori == ORIENTATION_UNKNOWN {
        unknown |= 1 << (NUM_CORNERS - 1);
    }
    (coord, unknown)
}

/// Decode a CO coord back into 8 corner orientations (the 8th is the
/// inverse-sum mod 3). Used for verifying / BFS reconstruction.
pub fn decode_corner_ori_coord(coord: u32) -> [u8; NUM_CORNERS] {
    let mut oris = [0u8; NUM_CORNERS];
    let mut c = coord;
    for i in (0..(NUM_CORNERS - 1)).rev() {
        oris[i] = (c % 3) as u8;
        c /= 3;
    }
    // 8th corner: -(sum of others) mod 3
    let s: u32 = oris[..(NUM_CORNERS - 1)].iter().map(|&x| x as u32).sum();
    oris[NUM_CORNERS - 1] = ((3 - (s % 3)) % 3) as u8;
    oris
}

/// Compute the edge orientation coordinate. Returns (coord, unknown_mask).
/// 12 edges, first 11 encoded in base 2.
pub fn edge_ori_coord(state: &CubieState) -> (u32, u16) {
    let mut coord: u32 = 0;
    let mut unknown: u16 = 0;
    for i in 0..(NUM_EDGES - 1) {
        let ori = state.edges[i].ori;
        if ori == ORIENTATION_UNKNOWN {
            unknown |= 1 << i;
            continue;
        }
        coord = coord * 2 + (ori as u32);
    }
    if state.edges[NUM_EDGES - 1].ori == ORIENTATION_UNKNOWN {
        unknown |= 1 << (NUM_EDGES - 1);
    }
    (coord, unknown)
}

/// Decode an EO coord into 12 edge orientations.
pub fn decode_edge_ori_coord(coord: u32) -> [u8; NUM_EDGES] {
    let mut oris = [0u8; NUM_EDGES];
    let mut c = coord;
    for i in (0..(NUM_EDGES - 1)).rev() {
        oris[i] = (c % 2) as u8;
        c /= 2;
    }
    let s: u32 = oris[..(NUM_EDGES - 1)].iter().map(|&x| x as u32).sum();
    oris[NUM_EDGES - 1] = ((2 - (s % 2)) % 2) as u8;
    oris
}

/// UD-slice coordinate: which 4 of 12 edge slots contain the FR/FL/BL/BR
/// edges (indices 8..12). Returns (coord, unknown_mask).
pub fn slice_coord(state: &CubieState) -> (u32, u16) {
    let mut unknown: u16 = 0;
    let mut is_slice_edge = [false; NUM_EDGES];
    let mut has_unknown_pos = false;
    for slot in 0..NUM_EDGES {
        let pos = state.edges[slot].pos;
        if pos == POSITION_UNKNOWN {
            unknown |= 1 << slot;
            has_unknown_pos = true;
            continue;
        }
        // Slice edges are indices 8..12 (FR, FL, BL, BR)
        if pos >= 8 && pos < 12 {
            is_slice_edge[slot] = true;
        }
    }
    if has_unknown_pos {
        return (0, unknown);
    }
    // Lehmer / combination index of the 4 slice positions in 12 slots
    let mut coord = 0u32;
    let mut k = 4u32;
    for i in (0..NUM_EDGES).rev() {
        if is_slice_edge[i] {
            if k > 0 {
                coord += binomial(i as u32, k);
                k -= 1;
            }
        }
    }
    // Adjust: standard formulation is "choose positions in sorted order from 0..12".
    // Above loop walks high-to-low, summing C(i, k_remaining) when seen.
    (coord, unknown)
}

fn binomial(n: u32, k: u32) -> u32 {
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

/// Lehmer index of the 8 corner positions (= corner permutation coord).
pub fn corner_perm_coord(state: &CubieState) -> (u32, u8) {
    let mut unknown: u8 = 0;
    let mut perm = [0u8; NUM_CORNERS];
    for slot in 0..NUM_CORNERS {
        let p = state.corners[slot].pos;
        if p == POSITION_UNKNOWN {
            unknown |= 1 << slot;
        }
        perm[slot] = p;
    }
    if unknown != 0 {
        return (0, unknown);
    }
    // Lehmer code
    let mut coord = 0u32;
    for i in 0..NUM_CORNERS {
        let mut cnt = 0u32;
        for j in (i + 1)..NUM_CORNERS {
            if perm[j] < perm[i] {
                cnt += 1;
            }
        }
        coord = coord * (NUM_CORNERS - i) as u32 + cnt;
    }
    (coord, unknown)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cubie::CubieState;

    #[test]
    fn solved_state_has_known_coords() {
        let s = CubieState::solved();
        let (co, co_unk) = corner_ori_coord(&s);
        assert_eq!(co, 0);
        assert_eq!(co_unk, 0);
        let (eo, eo_unk) = edge_ori_coord(&s);
        assert_eq!(eo, 0);
        assert_eq!(eo_unk, 0);
        // Colex convention: solved slice edges at positions {8,9,10,11}
        // gives C(8,1)+C(9,2)+C(10,3)+C(11,4) = 8+36+120+330 = 494
        // (the maximum). 0 corresponds to slice edges at {0,1,2,3}.
        let (sl, sl_unk) = slice_coord(&s);
        assert_eq!(sl, 494);
        assert_eq!(sl_unk, 0);
        let (cp, cp_unk) = corner_perm_coord(&s);
        assert_eq!(cp, 0);
        assert_eq!(cp_unk, 0);
    }

    #[test]
    fn decode_round_trips() {
        for coord in [0u32, 1, 100, 1000, 2186] {
            let oris = decode_corner_ori_coord(coord);
            let mut state = CubieState::solved();
            for i in 0..NUM_CORNERS {
                state.corners[i].ori = oris[i];
            }
            let (recovered, _) = corner_ori_coord(&state);
            assert_eq!(recovered, coord, "CO round-trip failed for {}", coord);
        }
        for coord in [0u32, 1, 100, 1000, 2047] {
            let oris = decode_edge_ori_coord(coord);
            let mut state = CubieState::solved();
            for i in 0..NUM_EDGES {
                state.edges[i].ori = oris[i];
            }
            let (recovered, _) = edge_ori_coord(&state);
            assert_eq!(recovered, coord, "EO round-trip failed for {}", coord);
        }
    }

    #[test]
    fn unknown_slots_set_mask() {
        let mut s = CubieState::solved();
        s.corners[0].ori = ORIENTATION_UNKNOWN;
        s.corners[3].ori = ORIENTATION_UNKNOWN;
        let (_, unk) = corner_ori_coord(&s);
        assert_eq!(unk, 0b00001001);
    }
}
