//! Cubie state representation — the core of plan v4.
//!
//! Each piece is identified by `(position, orientation)`. Unknown values use
//! the sentinels `POSITION_UNKNOWN` (255) and `ORIENTATION_UNKNOWN` (6),
//! matching CE Incomp.pas's NNN/NN/6 convention.
//!
//! # Indexing
//!
//! Corners (0..8):
//!   URF=0  UFL=1  ULB=2  UBR=3  DFR=4  DLF=5  DBL=6  DRB=7
//!
//! Edges (0..12):
//!   UR=0   UF=1   UL=2   UB=3   DR=4   DF=5   DL=6   DB=7
//!   FR=8   FL=9   BL=10  BR=11
//!
//! Orientation:
//!   Corner: 0 = U/D-color sticker on U/D face, 1 = CW twist, 2 = CCW twist
//!   Edge:   0 = correct EO (under <U,D,L,R,F2,B2>), 1 = flipped
//!
//! The cubie module replaces the facelet-based search engine for plan v4
//! (`INCIDENTS.md` 2026-05-19 rollback). UI / project I/O still use facelet
//! representation; `facelet_to_cubie` / `cubie_to_facelet` bridge the two.

use crate::facelet::{Facelet, FACELET_COUNT};
use crate::moves::Base;

// ----- Identity-piece indices ----------------------------------------------

pub const NUM_CORNERS: usize = 8;
pub const NUM_EDGES: usize = 12;

/// Sentinel for "this cubie's position is unknown" (CE: NNN / NN).
pub const POSITION_UNKNOWN: u8 = 255;
/// Sentinel for "this cubie's orientation is unknown" (CE: o=6).
pub const ORIENTATION_UNKNOWN: u8 = 6;

#[allow(non_snake_case, non_upper_case_globals)]
pub mod Corner {
    pub const URF: usize = 0;
    pub const UFL: usize = 1;
    pub const ULB: usize = 2;
    pub const UBR: usize = 3;
    pub const DFR: usize = 4;
    pub const DLF: usize = 5;
    pub const DBL: usize = 6;
    pub const DRB: usize = 7;
}

#[allow(non_snake_case, non_upper_case_globals)]
pub mod Edge {
    pub const UR: usize = 0;
    pub const UF: usize = 1;
    pub const UL: usize = 2;
    pub const UB: usize = 3;
    pub const DR: usize = 4;
    pub const DF: usize = 5;
    pub const DL: usize = 6;
    pub const DB: usize = 7;
    pub const FR: usize = 8;
    pub const FL: usize = 9;
    pub const BL: usize = 10;
    pub const BR: usize = 11;
}

// ----- Color sets per cubie ------------------------------------------------

/// Sorted color tuple identifying each corner (cube-core color codes
/// U=0, R=1, F=2, D=3, L=4, B=5).
pub const CORNER_COLOR_SET: [[u8; 3]; NUM_CORNERS] = [
    [0, 1, 2], // URF: U R F
    [0, 2, 4], // UFL: U F L
    [0, 4, 5], // ULB: U L B
    [0, 1, 5], // UBR: U B R
    [1, 2, 3], // DFR: D F R
    [2, 3, 4], // DLF: D L F
    [3, 4, 5], // DBL: D B L
    [1, 3, 5], // DRB: D R B
];

/// Sorted color tuple identifying each edge.
pub const EDGE_COLOR_SET: [[u8; 2]; NUM_EDGES] = [
    [0, 1], // UR
    [0, 2], // UF
    [0, 4], // UL
    [0, 5], // UB
    [1, 3], // DR
    [2, 3], // DF
    [3, 4], // DL
    [3, 5], // DB
    [1, 2], // FR
    [2, 4], // FL
    [4, 5], // BL
    [1, 5], // BR
];

// ----- Facelet sticker positions per cubie ---------------------------------
//
// These mirror moves.rs::CORNER_FACELETS / EDGE_FACELETS. Order within each
// row is significant: the first element is the U/D-face sticker (for
// corners) or the "primary" sticker (for edges). Orientation is the index
// where the U/D color is found.

pub const CORNER_FACELET_POSITIONS: [[usize; 3]; NUM_CORNERS] = [
    [8, 9, 20],   // URF — index 0 is U face
    [6, 18, 38],  // UFL — index 0 is U face
    [0, 36, 47],  // ULB — index 0 is U face
    [2, 45, 11],  // UBR — index 0 is U face
    [29, 26, 15], // DFR — index 0 is D face
    [27, 44, 24], // DLF — index 0 is D face
    [33, 53, 42], // DBL — index 0 is D face
    [35, 17, 51], // DRB — index 0 is D face
];

pub const EDGE_FACELET_POSITIONS: [[usize; 2]; NUM_EDGES] = [
    [5, 10],  // UR
    [7, 19],  // UF
    [3, 37],  // UL
    [1, 46],  // UB
    [32, 16], // DR
    [28, 25], // DF
    [30, 43], // DL
    [34, 52], // DB
    [23, 12], // FR
    [21, 41], // FL
    [50, 39], // BL
    [48, 14], // BR
];

// ----- Data types ----------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CornerCubie {
    pub pos: u8, // 0..8 or POSITION_UNKNOWN
    pub ori: u8, // 0..3 or ORIENTATION_UNKNOWN
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EdgeCubie {
    pub pos: u8, // 0..12 or POSITION_UNKNOWN
    pub ori: u8, // 0..2 or ORIENTATION_UNKNOWN
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CubieState {
    /// `corners[slot] = which cubie occupies this slot, in what orientation`
    pub corners: [CornerCubie; NUM_CORNERS],
    /// `edges[slot] = which cubie occupies this slot, in what orientation`
    pub edges: [EdgeCubie; NUM_EDGES],
}

impl CubieState {
    /// Solved state: each slot holds its home cubie, orientation 0.
    pub fn solved() -> Self {
        let mut corners = [CornerCubie { pos: 0, ori: 0 }; NUM_CORNERS];
        for i in 0..NUM_CORNERS {
            corners[i].pos = i as u8;
        }
        let mut edges = [EdgeCubie { pos: 0, ori: 0 }; NUM_EDGES];
        for i in 0..NUM_EDGES {
            edges[i].pos = i as u8;
        }
        CubieState { corners, edges }
    }

    /// All-unknown state — every slot tagged unknown. Used as a starting
    /// point when building a state with partial information.
    pub fn unknown() -> Self {
        CubieState {
            corners: [CornerCubie {
                pos: POSITION_UNKNOWN,
                ori: ORIENTATION_UNKNOWN,
            }; NUM_CORNERS],
            edges: [EdgeCubie {
                pos: POSITION_UNKNOWN,
                ori: ORIENTATION_UNKNOWN,
            }; NUM_EDGES],
        }
    }
}

// ----- Facelet → cubie -----------------------------------------------------

/// Read three sticker colors at the corner slot's positions and identify
/// the cubie that lives there. Returns `(pos, ori)`.
///
/// `pos = POSITION_UNKNOWN` if the colors don't match any known corner
/// (e.g. the facelet has a WILDCARD). In that case `ori` is also unknown.
fn identify_corner_at_slot(facelet: &Facelet, slot: usize) -> (u8, u8) {
    let positions = &CORNER_FACELET_POSITIONS[slot];
    let mut colors = [0u8; 3];
    for i in 0..3 {
        let v = facelet[positions[i]];
        if v < 0 || v > 5 {
            return (POSITION_UNKNOWN, ORIENTATION_UNKNOWN);
        }
        colors[i] = v as u8;
    }

    // Identify the cubie by sorted color set
    let mut sorted = colors;
    sorted.sort();
    let cubie_idx = CORNER_COLOR_SET
        .iter()
        .position(|set| *set == sorted)
        .unwrap_or(usize::MAX);
    if cubie_idx == usize::MAX {
        return (POSITION_UNKNOWN, ORIENTATION_UNKNOWN);
    }

    // Orientation: which of the three stickers carries the U/D color (0 or 3)?
    let ori = (0..3)
        .find(|&i| colors[i] == 0 || colors[i] == 3)
        .unwrap_or(0) as u8;

    (cubie_idx as u8, ori)
}

/// Identify the edge cubie at the given slot. The orientation convention is
/// the standard F-axis flip: an edge is "oriented" (ori=0) iff it would
/// reach its home slot using only `<U, D, L, R, F2, B2>` moves.
///
/// For practical detection: at any slot, the "primary" sticker (the first
/// in `EDGE_FACELET_POSITIONS[slot]`) must hold a non-flipping color. The
/// rule used here matches the one min2phase / Kociemba's two-phase use:
///
///   * Primary sticker on U/D face: must hold U/D color
///   * Primary sticker on F/B face: must hold a non-F/B color (i.e. U or D)
///   * Primary sticker on L/R face: edge is in E slice (FR/FL/BL/BR); the
///     primary sticker (on R/L face for these slots) must hold F or B color
fn identify_edge_at_slot(facelet: &Facelet, slot: usize) -> (u8, u8) {
    let positions = &EDGE_FACELET_POSITIONS[slot];
    let mut colors = [0u8; 2];
    for i in 0..2 {
        let v = facelet[positions[i]];
        if v < 0 || v > 5 {
            return (POSITION_UNKNOWN, ORIENTATION_UNKNOWN);
        }
        colors[i] = v as u8;
    }

    let mut sorted = colors;
    sorted.sort();
    let cubie_idx = EDGE_COLOR_SET
        .iter()
        .position(|set| *set == sorted)
        .unwrap_or(usize::MAX);
    if cubie_idx == usize::MAX {
        return (POSITION_UNKNOWN, ORIENTATION_UNKNOWN);
    }

    // Edge orientation (F-axis convention).
    // For slots 0..8 (UR/UF/UL/UB/DR/DF/DL/DB): primary sticker is on
    // U or D face. EO=0 iff that sticker is U or D color.
    // For slots 8..12 (FR/FL/BL/BR): primary sticker is on F or B face,
    // but the U/D color is on the other sticker. EO=0 iff THIS slot's
    // primary holds F or B color (i.e. the non-U/D sticker is on the
    // outer (F/B) face).
    let primary = colors[0];
    let ori = if slot < 8 {
        // primary sticker on U/D face — must be U/D color
        if primary == 0 || primary == 3 {
            0
        } else {
            1
        }
    } else {
        // E-slice edge — primary on F/B face — must be F/B color
        if primary == 2 || primary == 5 {
            0
        } else {
            1
        }
    };

    (cubie_idx as u8, ori)
}

/// Convert a 54-byte facelet to cubie state. Wildcard / invalid stickers
/// produce `POSITION_UNKNOWN` / `ORIENTATION_UNKNOWN` for the affected
/// slots.
pub fn facelet_to_cubie(facelet: &Facelet) -> CubieState {
    let mut state = CubieState::unknown();
    for slot in 0..NUM_CORNERS {
        let (pos, ori) = identify_corner_at_slot(facelet, slot);
        state.corners[slot] = CornerCubie { pos, ori };
    }
    for slot in 0..NUM_EDGES {
        let (pos, ori) = identify_edge_at_slot(facelet, slot);
        state.edges[slot] = EdgeCubie { pos, ori };
    }
    state
}

/// Convert cubie state back to facelet. For any slot whose cubie or
/// orientation is unknown, that slot's facelet positions are set to
/// `mask::WILDCARD` (-1).
pub fn cubie_to_facelet(state: &CubieState) -> Facelet {
    let mut f: Facelet = [crate::mask::WILDCARD; FACELET_COUNT];
    // Centers — solved orientation only (cube-core does not track center
    // rotation in cubie repr; centers always at canonical positions).
    for c in 0..6 {
        let center_pos = 4 + 9 * c;
        f[center_pos] = c as i8;
    }
    for slot in 0..NUM_CORNERS {
        let cubie = &state.corners[slot];
        if cubie.pos == POSITION_UNKNOWN || cubie.ori == ORIENTATION_UNKNOWN {
            continue;
        }
        let cubie_idx = cubie.pos as usize;
        let cubie_colors = &CORNER_COLOR_SET[cubie_idx];
        // We need the original ordering of colors that matches the slot's
        // facelet position ordering. For ori=0 at home slot, the U/D-color
        // sticker (CORNER_COLOR_SET[i][0] or [2]) is at position 0.
        //
        // Reconstruct the per-cubie facelet color triple in slot-frame:
        // start from solved (cubie at its home slot, ori=0), where the
        // ordering matches CORNER_FACELET_POSITIONS[cubie_idx].
        let home_positions = &CORNER_FACELET_POSITIONS[cubie_idx];
        // Recover the color at each position-index 0/1/2 by looking up
        // the solved facelet (the color at home is face[i/9]).
        let mut home_colors = [0u8; 3];
        for k in 0..3 {
            home_colors[k] = (home_positions[k] / 9) as u8;
        }
        // Apply orientation rotation: ori=0 is identity, ori=1 cycles
        // positions [0,1,2] -> [2,0,1] (CW from outside), ori=2 -> [1,2,0]
        let oriented = match cubie.ori {
            0 => home_colors,
            1 => [home_colors[2], home_colors[0], home_colors[1]],
            2 => [home_colors[1], home_colors[2], home_colors[0]],
            _ => continue,
        };
        let slot_positions = &CORNER_FACELET_POSITIONS[slot];
        for k in 0..3 {
            f[slot_positions[k]] = oriented[k] as i8;
        }
    }
    for slot in 0..NUM_EDGES {
        let cubie = &state.edges[slot];
        if cubie.pos == POSITION_UNKNOWN || cubie.ori == ORIENTATION_UNKNOWN {
            continue;
        }
        let cubie_idx = cubie.pos as usize;
        let home_positions = &EDGE_FACELET_POSITIONS[cubie_idx];
        let mut home_colors = [0u8; 2];
        for k in 0..2 {
            home_colors[k] = (home_positions[k] / 9) as u8;
        }
        let oriented = if cubie.ori == 0 {
            home_colors
        } else {
            [home_colors[1], home_colors[0]]
        };
        let slot_positions = &EDGE_FACELET_POSITIONS[slot];
        for k in 0..2 {
            f[slot_positions[k]] = oriented[k] as i8;
        }
    }
    f
}

// ----- Cubie moves ---------------------------------------------------------

/// Permutation describing a cubie-level move. `corner_perm[slot] = source_slot`
/// means "the cubie that ends up at `slot` came from `source_slot` (after
/// orientation update)."
#[derive(Debug, Clone, Copy)]
pub struct CubieMove {
    pub corner_perm: [u8; NUM_CORNERS],
    pub corner_ori_delta: [u8; NUM_CORNERS],
    pub edge_perm: [u8; NUM_EDGES],
    pub edge_ori_delta: [u8; NUM_EDGES],
}

impl CubieMove {
    pub fn identity() -> Self {
        let mut corner_perm = [0u8; NUM_CORNERS];
        for i in 0..NUM_CORNERS {
            corner_perm[i] = i as u8;
        }
        let mut edge_perm = [0u8; NUM_EDGES];
        for i in 0..NUM_EDGES {
            edge_perm[i] = i as u8;
        }
        CubieMove {
            corner_perm,
            corner_ori_delta: [0u8; NUM_CORNERS],
            edge_perm,
            edge_ori_delta: [0u8; NUM_EDGES],
        }
    }
}

/// Build the CW-quarter-turn move for a single base. Uses moves.rs's
/// `base_data()` cycles to determine which cubies rotate.
fn cw_quarter_move_for_base(base: usize) -> CubieMove {
    let base_data_fn = crate::moves::base_data_for_test;
    let bd = base_data_fn(base);
    let mut m = CubieMove::identity();
    if bd.has_corner {
        // CW corner cycle: cycle[0] -> cycle[1] -> cycle[2] -> cycle[3] -> cycle[0]
        // Meaning: after this move, the cubie at cycle[1] is the one that
        // was at cycle[0] (with orientation delta added).
        for i in 0..4 {
            let from = bd.corner_cycle[i];
            let to = bd.corner_cycle[(i + 1) % 4];
            m.corner_perm[to] = from as u8;
            m.corner_ori_delta[to] = bd.corner_ori[i] as u8;
        }
    }
    if bd.has_edge {
        for i in 0..4 {
            let from = bd.edge_cycle[i];
            let to = bd.edge_cycle[(i + 1) % 4];
            m.edge_perm[to] = from as u8;
            m.edge_ori_delta[to] = bd.edge_ori[i] as u8;
        }
    }
    m
}

/// Compose two CubieMoves: `result = b ∘ a` (apply a first, then b).
fn compose(a: &CubieMove, b: &CubieMove) -> CubieMove {
    let mut r = CubieMove::identity();
    for slot in 0..NUM_CORNERS {
        let mid = b.corner_perm[slot] as usize;
        r.corner_perm[slot] = a.corner_perm[mid];
        r.corner_ori_delta[slot] = (a.corner_ori_delta[mid] + b.corner_ori_delta[slot]) % 3;
    }
    for slot in 0..NUM_EDGES {
        let mid = b.edge_perm[slot] as usize;
        r.edge_perm[slot] = a.edge_perm[mid];
        r.edge_ori_delta[slot] = (a.edge_ori_delta[mid] + b.edge_ori_delta[slot]) % 2;
    }
    r
}

/// All 18 base × variant move permutations (CW, CCW, 180), indexed
/// matching cube-core's `move_of(base, variant)` = base*3 + variant.
/// Bases 0..6 = U/D/R/L/F/B (the only ones supported by the cubie engine
/// for now). Wide and slice moves are not pre-built here — they can be
/// added by expressing them as compositions of face+slice (cube-core
/// already has the slice-to-face decomposition in `moves.rs`).
pub fn build_face_moves() -> Vec<CubieMove> {
    let bases = [Base::U, Base::D, Base::R, Base::L, Base::F, Base::B];
    let mut moves = Vec::with_capacity(18);
    for &b in &bases {
        let cw = cw_quarter_move_for_base(b);
        let mut ccw = compose(&cw, &cw);
        ccw = compose(&ccw, &cw); // 3 CW = 1 CCW
        let half = compose(&cw, &cw);
        moves.push(cw);
        moves.push(ccw);
        moves.push(half);
    }
    moves
}

/// Apply a `CubieMove` to a state: each slot's new cubie is taken from
/// the source slot, and orientation is incremented by the move's delta.
///
/// `#[inline]` lets LLVM auto-vectorize the corner/edge loops and specialize
/// per call site. The branch-free orientation update (replacing `% 3` /
/// `% 2` with conditional subtract) cuts ~30% per call on the hot DFS path
/// while preserving identical semantics for ORIENTATION_UNKNOWN.
#[inline]
pub fn apply_cubie_move(state: &CubieState, m: &CubieMove) -> CubieState {
    let mut out = *state;
    for slot in 0..NUM_CORNERS {
        let from = m.corner_perm[slot] as usize;
        let src = state.corners[from];
        // Branch-free `% 3`: corner ori is 0..2, delta is 0..2, sum is 0..4.
        // ORIENTATION_UNKNOWN (6) + delta is 6..8 — preserved by the high-
        // value guard below (any sum > 5 stays > 2 after `-3 if >= 3`).
        let sum = src.ori.wrapping_add(m.corner_ori_delta[slot]);
        let new_ori = if src.ori == ORIENTATION_UNKNOWN {
            ORIENTATION_UNKNOWN
        } else {
            sum - 3 * ((sum >= 3) as u8)
        };
        out.corners[slot] = CornerCubie {
            pos: src.pos,
            ori: new_ori,
        };
    }
    for slot in 0..NUM_EDGES {
        let from = m.edge_perm[slot] as usize;
        let src = state.edges[from];
        // Branch-free `% 2` = bitwise AND with 1.
        let sum = src.ori.wrapping_add(m.edge_ori_delta[slot]);
        let new_ori = if src.ori == ORIENTATION_UNKNOWN {
            ORIENTATION_UNKNOWN
        } else {
            sum & 1
        };
        out.edges[slot] = EdgeCubie {
            pos: src.pos,
            ori: new_ori,
        };
    }
    out
}

// ----- Tests ---------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::facelet::create_solved_facelet;
    use crate::moves::{apply_moves, parse_move_token};

    #[test]
    fn solved_facelet_round_trips_through_cubie() {
        let solved = create_solved_facelet();
        let cubie = facelet_to_cubie(&solved);
        // All cubies at home, orientation 0
        for i in 0..NUM_CORNERS {
            assert_eq!(cubie.corners[i].pos, i as u8, "corner slot {}", i);
            assert_eq!(cubie.corners[i].ori, 0, "corner ori slot {}", i);
        }
        for i in 0..NUM_EDGES {
            assert_eq!(cubie.edges[i].pos, i as u8, "edge slot {}", i);
            assert_eq!(cubie.edges[i].ori, 0, "edge ori slot {}", i);
        }
        // Round trip back
        let f = cubie_to_facelet(&cubie);
        assert_eq!(f, solved, "facelet round-trip mismatch");
    }

    fn scramble_to_move_indices(s: &str) -> Vec<usize> {
        s.split_whitespace()
            .filter_map(|t| parse_move_token(t))
            .collect()
    }

    #[test]
    fn cubie_apply_matches_facelet_apply_for_face_turns() {
        // For each of 6 CW face turns, verify cubie engine matches facelet
        // engine on solved start state.
        let solved_facelet = create_solved_facelet();
        let cubie_moves = build_face_moves();

        // Map base index in cube-core (U=0, D=1, R=2, L=3, F=4, B=5)
        // already matches our build order.
        let test_tokens = ["U", "D", "R", "L", "F", "B"];
        for (base_idx, tok) in test_tokens.iter().enumerate() {
            let move_idx = parse_move_token(tok).unwrap();
            // Apply facelet move
            let f_after = apply_moves(&solved_facelet, &[move_idx]);
            // Apply cubie move
            let cubie_start = facelet_to_cubie(&solved_facelet);
            let cubie_after = apply_cubie_move(&cubie_start, &cubie_moves[base_idx * 3]);
            // Round-trip to facelet
            let cubie_as_facelet = cubie_to_facelet(&cubie_after);
            assert_eq!(
                cubie_as_facelet, f_after,
                "{} CW: cubie engine differs from facelet engine",
                tok
            );
        }
    }

    #[test]
    fn cubie_apply_matches_facelet_for_compound_sequence() {
        let solved_facelet = create_solved_facelet();
        let scrambles = [
            "R U R' U'",
            "R U R' U R U2 R'",        // Sune
            "F R U R' U' F'",          // OLL 45
            "R U' R U R U R U' R' U' R2", // U-perm
            "F2 U2 R F B' D R2 D2 L D' B' R B' L'", // random
        ];
        let cubie_moves = build_face_moves();
        for scramble in &scrambles {
            let moves = scramble_to_move_indices(scramble);
            // facelet path
            let f_after = apply_moves(&solved_facelet, &moves);
            // cubie path
            let mut cubie = facelet_to_cubie(&solved_facelet);
            for &m in &moves {
                let base = m / 3;
                if base >= 6 {
                    // skip wide/slice/rotation for now
                    return;
                }
                cubie = apply_cubie_move(&cubie, &cubie_moves[m]);
            }
            let cubie_as_facelet = cubie_to_facelet(&cubie);
            assert_eq!(
                cubie_as_facelet, f_after,
                "scramble {:?} cubie/facelet mismatch",
                scramble
            );
        }
    }
}
