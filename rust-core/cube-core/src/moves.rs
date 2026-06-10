//! 3x3 Rubik's cube moves on the facelet representation.
//!
//! Move encoding: integer in [0, MOVE_COUNT).
//!   Base move index: U, D, R, L, F, B, Uw, Dw, Rw, Lw, Fw, Bw, M, E, S  (0..14)
//!   Variant: 0=CW, 1=CCW (prime), 2=180
//!   Move = base * 3 + variant
//!
//! Each move has a precomputed permutation `MOVE_PERM[move]` such that:
//!   new[i] = old[MOVE_PERM[move][i]]

use std::sync::OnceLock;

use crate::facelet::FACELET_COUNT;

pub const MOVE_COUNT: usize = 54;
pub const BASE_COUNT: usize = 18;
/// Number of "face/wide/slice" bases (0..15). The remaining 3 (15..18) are
/// whole-cube rotations x/y/z, which physically relabel the cube without
/// changing the underlying state. Coord-based pruning only knows the first
/// 45 moves; the engine guards rotation moves with `is_rotation_base`.
pub const NON_ROTATION_BASE_COUNT: usize = 15;
pub const NON_ROTATION_MOVE_COUNT: usize = NON_ROTATION_BASE_COUNT * 3;

#[allow(non_snake_case, non_upper_case_globals)]
pub mod Base {
    pub const U: usize = 0;
    pub const D: usize = 1;
    pub const R: usize = 2;
    pub const L: usize = 3;
    pub const F: usize = 4;
    pub const B: usize = 5;
    pub const Uw: usize = 6;
    pub const Dw: usize = 7;
    pub const Rw: usize = 8;
    pub const Lw: usize = 9;
    pub const Fw: usize = 10;
    pub const Bw: usize = 11;
    pub const M: usize = 12;
    pub const E: usize = 13;
    pub const S: usize = 14;
    /// Whole-cube rotation around the R-axis (CW from R side). `x = Rw L'`.
    pub const X: usize = 15;
    /// Whole-cube rotation around the U-axis (CW from U side). `y = Uw D'`.
    pub const Y: usize = 16;
    /// Whole-cube rotation around the F-axis (CW from F side). `z = Fw B'`.
    pub const Z: usize = 17;
}

pub const BASE_NAMES: [&str; BASE_COUNT] = [
    "U", "D", "R", "L", "F", "B", "Uw", "Dw", "Rw", "Lw", "Fw", "Bw", "M", "E", "S", "x", "y", "z",
];

#[inline]
pub fn is_rotation_base(base: usize) -> bool {
    base >= NON_ROTATION_BASE_COUNT
}

#[inline]
pub fn is_rotation_move(m: usize) -> bool {
    is_rotation_base(move_base(m))
}

pub const VARIANT_SUFFIX: [&str; 3] = ["", "'", "2"];

#[inline]
pub fn move_of(base: usize, variant: usize) -> usize {
    base * 3 + variant
}
#[inline]
pub fn move_base(m: usize) -> usize {
    m / 3
}
#[inline]
pub fn move_variant(m: usize) -> usize {
    m % 3
}
pub fn move_name(m: usize) -> String {
    format!("{}{}", BASE_NAMES[move_base(m)], VARIANT_SUFFIX[move_variant(m)])
}

pub fn inverse_move(m: usize) -> usize {
    let b = move_base(m);
    let v = move_variant(m);
    if v == 2 {
        m
    } else {
        b * 3 + (1 - v)
    }
}

// ============================================================
// Cubie data for generating move permutations.
// ============================================================

const CORNER_FACELETS: [[usize; 3]; 8] = [
    [8, 9, 20],   // URF
    [6, 18, 38],  // UFL
    [0, 36, 47],  // ULB
    [2, 45, 11],  // UBR
    [29, 26, 15], // DFR
    [27, 44, 24], // DLF
    [33, 53, 42], // DBL
    [35, 17, 51], // DRB
];

const EDGE_FACELETS: [[usize; 2]; 12] = [
    [5, 10],   // UR
    [7, 19],   // UF
    [3, 37],   // UL
    [1, 46],   // UB
    [32, 16],  // DR
    [28, 25],  // DF
    [30, 43],  // DL
    [34, 52],  // DB
    [23, 12],  // FR
    [21, 41],  // FL
    [50, 39],  // BL
    [48, 14],  // BR
];

const CENTER_INDEX: [usize; 6] = [4, 13, 22, 31, 40, 49];

#[derive(Default, Clone, Copy)]
pub struct BaseMoveData {
    pub has_corner: bool,
    pub corner_cycle: [usize; 4],
    pub corner_ori: [usize; 4],
    pub has_edge: bool,
    pub edge_cycle: [usize; 4],
    pub edge_ori: [usize; 4],
    pub has_face_rot: bool,
    pub face_rotation: usize,
    pub has_center: bool,
    pub center_cycle: [usize; 4],
}

/// Public accessor for `base_data()` — used by the cubie engine to build
/// piece-level move tables from the same canonical cycles that the
/// facelet engine uses.
pub fn base_data_for_test(base: usize) -> BaseMoveData {
    base_data()[base]
}

fn base_data() -> [BaseMoveData; BASE_COUNT] {
    let mut d = [BaseMoveData::default(); BASE_COUNT];
    // U
    d[Base::U] = BaseMoveData {
        has_face_rot: true,
        face_rotation: 0,
        has_corner: true,
        corner_cycle: [0, 1, 2, 3],
        corner_ori: [0, 0, 0, 0],
        has_edge: true,
        edge_cycle: [1, 2, 3, 0],
        edge_ori: [0, 0, 0, 0],
        has_center: false,
        center_cycle: [0; 4],
    };
    // D
    d[Base::D] = BaseMoveData {
        has_face_rot: true,
        face_rotation: 3,
        has_corner: true,
        corner_cycle: [4, 7, 6, 5],
        corner_ori: [0, 0, 0, 0],
        has_edge: true,
        edge_cycle: [5, 4, 7, 6],
        edge_ori: [0, 0, 0, 0],
        has_center: false,
        center_cycle: [0; 4],
    };
    // R
    d[Base::R] = BaseMoveData {
        has_face_rot: true,
        face_rotation: 1,
        has_corner: true,
        corner_cycle: [0, 3, 7, 4],
        corner_ori: [1, 2, 1, 2],
        has_edge: true,
        edge_cycle: [0, 11, 4, 8],
        edge_ori: [0, 0, 0, 0],
        has_center: false,
        center_cycle: [0; 4],
    };
    // L
    d[Base::L] = BaseMoveData {
        has_face_rot: true,
        face_rotation: 4,
        has_corner: true,
        corner_cycle: [1, 5, 6, 2],
        corner_ori: [2, 1, 2, 1],
        has_edge: true,
        edge_cycle: [2, 9, 6, 10],
        edge_ori: [0, 0, 0, 0],
        has_center: false,
        center_cycle: [0; 4],
    };
    // F
    d[Base::F] = BaseMoveData {
        has_face_rot: true,
        face_rotation: 2,
        has_corner: true,
        corner_cycle: [0, 4, 5, 1],
        corner_ori: [2, 1, 2, 1],
        has_edge: true,
        edge_cycle: [1, 8, 5, 9],
        edge_ori: [1, 1, 1, 1],
        has_center: false,
        center_cycle: [0; 4],
    };
    // B
    d[Base::B] = BaseMoveData {
        has_face_rot: true,
        face_rotation: 5,
        has_corner: true,
        corner_cycle: [3, 2, 6, 7],
        corner_ori: [1, 2, 1, 2],
        has_edge: true,
        edge_cycle: [3, 10, 7, 11],
        edge_ori: [1, 1, 1, 1],
        has_center: false,
        center_cycle: [0; 4],
    };
    // M slice
    d[Base::M] = BaseMoveData {
        has_edge: true,
        edge_cycle: [1, 5, 7, 3],
        edge_ori: [1, 1, 1, 1],
        has_center: true,
        center_cycle: [0, 2, 3, 5],
        ..BaseMoveData::default()
    };
    // E slice
    d[Base::E] = BaseMoveData {
        has_edge: true,
        edge_cycle: [8, 11, 10, 9],
        edge_ori: [1, 1, 1, 1],
        has_center: true,
        center_cycle: [2, 1, 5, 4],
        ..BaseMoveData::default()
    };
    // S slice
    d[Base::S] = BaseMoveData {
        has_edge: true,
        edge_cycle: [0, 4, 6, 2],
        edge_ori: [1, 1, 1, 1],
        has_center: true,
        center_cycle: [0, 1, 3, 4],
        ..BaseMoveData::default()
    };
    d
}

fn identity_perm() -> [i8; FACELET_COUNT] {
    let mut p = [0i8; FACELET_COUNT];
    for i in 0..FACELET_COUNT {
        p[i] = i as i8;
    }
    p
}

fn apply_face_rotation_cw(perm: &mut [i8; FACELET_COUNT], face: usize) {
    let base = CENTER_INDEX[face] - 4;
    perm[base + 2] = (base + 0) as i8;
    perm[base + 8] = (base + 2) as i8;
    perm[base + 6] = (base + 8) as i8;
    perm[base + 0] = (base + 6) as i8;
    perm[base + 5] = (base + 1) as i8;
    perm[base + 7] = (base + 5) as i8;
    perm[base + 3] = (base + 7) as i8;
    perm[base + 1] = (base + 3) as i8;
    perm[base + 4] = (base + 4) as i8;
}

fn apply_corner_cycle(perm: &mut [i8; FACELET_COUNT], cycle: &[usize; 4], ori: &[usize; 4]) {
    for i in 0..4 {
        let src = cycle[i];
        let dst = cycle[(i + 1) % 4];
        let o = ori[i];
        let sf = CORNER_FACELETS[src];
        let df = CORNER_FACELETS[dst];
        for k in 0..3 {
            perm[df[(k + o) % 3]] = sf[k] as i8;
        }
    }
}

fn apply_edge_cycle(perm: &mut [i8; FACELET_COUNT], cycle: &[usize; 4], ori: &[usize; 4]) {
    for i in 0..4 {
        let src = cycle[i];
        let dst = cycle[(i + 1) % 4];
        let o = ori[i];
        let sf = EDGE_FACELETS[src];
        let df = EDGE_FACELETS[dst];
        for k in 0..2 {
            perm[df[(k + o) % 2]] = sf[k] as i8;
        }
    }
}

fn apply_center_cycle(perm: &mut [i8; FACELET_COUNT], cycle: &[usize; 4]) {
    for i in 0..4 {
        let src = CENTER_INDEX[cycle[i]];
        let dst = CENTER_INDEX[cycle[(i + 1) % 4]];
        perm[dst] = src as i8;
    }
}

fn build_base_perm(data: &BaseMoveData) -> [i8; FACELET_COUNT] {
    let mut perm = identity_perm();
    if data.has_face_rot {
        apply_face_rotation_cw(&mut perm, data.face_rotation);
    }
    if data.has_corner {
        apply_corner_cycle(&mut perm, &data.corner_cycle, &data.corner_ori);
    }
    if data.has_edge {
        apply_edge_cycle(&mut perm, &data.edge_cycle, &data.edge_ori);
    }
    if data.has_center {
        apply_center_cycle(&mut perm, &data.center_cycle);
    }
    perm
}

/// Compose two permutations: result[i] = a[b[i]]. Apply b first, then a.
pub fn compose_perm(a: &[i8; FACELET_COUNT], b: &[i8; FACELET_COUNT]) -> [i8; FACELET_COUNT] {
    let mut out = [0i8; FACELET_COUNT];
    for i in 0..FACELET_COUNT {
        out[i] = a[b[i] as usize];
    }
    out
}

pub fn inverse_perm(p: &[i8; FACELET_COUNT]) -> [i8; FACELET_COUNT] {
    let mut inv = [0i8; FACELET_COUNT];
    for i in 0..FACELET_COUNT {
        inv[p[i] as usize] = i as i8;
    }
    inv
}

fn build_all_move_perms() -> Vec<[i8; FACELET_COUNT]> {
    let bd = base_data();
    let mut base_cw: Vec<Option<[i8; FACELET_COUNT]>> = vec![None; BASE_COUNT];
    for b in [
        Base::U, Base::D, Base::R, Base::L, Base::F, Base::B, Base::M, Base::E, Base::S,
    ] {
        base_cw[b] = Some(build_base_perm(&bd[b]));
    }
    let m = base_cw[Base::M].unwrap();
    let e = base_cw[Base::E].unwrap();
    let s = base_cw[Base::S].unwrap();
    let mi = inverse_perm(&m);
    let ei = inverse_perm(&e);
    let si = inverse_perm(&s);

    base_cw[Base::Uw] = Some(compose_perm(&base_cw[Base::U].unwrap(), &ei));
    base_cw[Base::Dw] = Some(compose_perm(&base_cw[Base::D].unwrap(), &e));
    base_cw[Base::Rw] = Some(compose_perm(&base_cw[Base::R].unwrap(), &mi));
    base_cw[Base::Lw] = Some(compose_perm(&base_cw[Base::L].unwrap(), &m));
    base_cw[Base::Fw] = Some(compose_perm(&base_cw[Base::F].unwrap(), &s));
    base_cw[Base::Bw] = Some(compose_perm(&base_cw[Base::B].unwrap(), &si));

    // Whole-cube rotations. x = Rw L', y = Uw D', z = Fw B'.
    let rw_cw = base_cw[Base::Rw].unwrap();
    let l_inv = inverse_perm(&base_cw[Base::L].unwrap());
    base_cw[Base::X] = Some(compose_perm(&rw_cw, &l_inv));
    let uw_cw = base_cw[Base::Uw].unwrap();
    let d_inv = inverse_perm(&base_cw[Base::D].unwrap());
    base_cw[Base::Y] = Some(compose_perm(&uw_cw, &d_inv));
    let fw_cw = base_cw[Base::Fw].unwrap();
    let b_inv = inverse_perm(&base_cw[Base::B].unwrap());
    base_cw[Base::Z] = Some(compose_perm(&fw_cw, &b_inv));

    let mut perms: Vec<[i8; FACELET_COUNT]> = Vec::with_capacity(MOVE_COUNT);
    perms.resize(MOVE_COUNT, [0i8; FACELET_COUNT]);
    for b in 0..BASE_COUNT {
        let cw = base_cw[b].unwrap();
        perms[b * 3] = cw;
        perms[b * 3 + 1] = inverse_perm(&cw);
        perms[b * 3 + 2] = compose_perm(&cw, &cw);
    }
    perms
}

static MOVE_PERM_CELL: OnceLock<Vec<[i8; FACELET_COUNT]>> = OnceLock::new();

pub fn move_perm() -> &'static [[i8; FACELET_COUNT]] {
    MOVE_PERM_CELL.get_or_init(build_all_move_perms).as_slice()
}

/// Static handle exposed for parity with TS naming.
pub struct MovePerm;

impl MovePerm {
    pub fn get(m: usize) -> &'static [i8; FACELET_COUNT] {
        &move_perm()[m]
    }
}

/// Compatibility re-export. Use `move_perm()` or `MovePerm::get(m)` directly.
#[allow(non_upper_case_globals)]
pub const MOVE_PERM: MovePerm = MovePerm;

pub fn apply_move(state: &[i8; FACELET_COUNT], m: usize) -> [i8; FACELET_COUNT] {
    let perm = &move_perm()[m];
    let mut out = [0i8; FACELET_COUNT];
    for i in 0..FACELET_COUNT {
        out[i] = state[perm[i] as usize];
    }
    out
}

pub fn apply_move_into(
    state: &[i8; FACELET_COUNT],
    m: usize,
    out: &mut [i8; FACELET_COUNT],
) {
    let perm = &move_perm()[m];
    for i in 0..FACELET_COUNT {
        out[i] = state[perm[i] as usize];
    }
}

pub fn apply_moves(state: &[i8; FACELET_COUNT], moves: &[usize]) -> [i8; FACELET_COUNT] {
    let mut a = *state;
    let mut b = [0i8; FACELET_COUNT];
    for &m in moves {
        apply_move_into(&a, m, &mut b);
        std::mem::swap(&mut a, &mut b);
    }
    a
}

// Axis classification for canonical pruning.
//
// Rotations get distinct axes (3/4/5) so they never collide with face/wide/
// slice moves under same-axis canonical pruning rules. Within a rotation
// axis, consecutive identical bases (e.g. `x x`) still collapse via the
// variant logic (canonical ordering inside an axis).
pub const AXIS_OF_BASE: [usize; BASE_COUNT] = [
    1, /* U */
    1, /* D */
    0, /* R */
    0, /* L */
    2, /* F */
    2, /* B */
    1, /* Uw */
    1, /* Dw */
    0, /* Rw */
    0, /* Lw */
    2, /* Fw */
    2, /* Bw */
    0, /* M */
    1, /* E */
    2, /* S */
    3, /* x */
    4, /* y */
    5, /* z */
];

pub fn move_axis(m: usize) -> usize {
    AXIS_OF_BASE[move_base(m)]
}

/// Parse a single WCA-style move token like `Rw'` or `U2`. Returns `None`
/// on unrecognized tokens.
pub fn parse_move_token(token: &str) -> Option<usize> {
    if token.is_empty() {
        return None;
    }
    // Normalize unicode prime variants to ascii.
    let t: String = token
        .chars()
        .map(|c| match c {
            '\u{2019}' | '\u{02BC}' | '\u{02B9}' | '\u{2032}' => '\'',
            other => other,
        })
        .collect();
    let bytes = t.as_bytes();
    let (base_str, rest): (&str, &str) = if bytes.len() >= 2 && bytes[1] == b'w' {
        (&t[..2], &t[2..])
    } else {
        (&t[..1], &t[1..])
    };
    let base_idx = BASE_NAMES.iter().position(|n| *n == base_str)?;
    let variant = match rest {
        "" => 0,
        "'" => 1,
        "2" => 2,
        _ => return None,
    };
    Some(base_idx * 3 + variant)
}
