//! Kociemba-style coordinate encoding for the pruning tables.
//!
//! Two coord families used by the search heuristic:
//!   * CO (corner orient) ∈ [0, 2187)
//!   * EO (edge   orient) ∈ [0, 2048)
//!   * slice (UD-slice occupancy) ∈ [0, 495)
//!   * CP (corner permutation) ∈ [0, 40320)
//!
//! Encoding mirrors the TS module `core/coord.ts` so that disk-cached BFS
//! tables built by either implementation are interchangeable.

use std::sync::OnceLock;

use crate::facelet::{create_solved_facelet, Color, FACELET_COUNT};
use crate::mask::WILDCARD;
use crate::moves::{apply_moves, MOVE_COUNT};

// Sticker positions per cubie in Kociemba convention.
pub(crate) const CORNER_FACELETS: [[usize; 3]; 8] = [
    [8, 9, 20],
    [6, 18, 38],
    [0, 36, 47],
    [2, 45, 11],
    [29, 26, 15],
    [27, 44, 24],
    [33, 53, 42],
    [35, 17, 51],
];

/// Public re-export of CORNER_FACELETS for use in sibling modules
/// (`subgroup.rs`) without exposing internal-only items.
pub const CORNER_FACELETS_PUB: [[usize; 3]; 8] = CORNER_FACELETS;

pub(crate) const EDGE_FACELETS: [[usize; 2]; 12] = [
    [5, 10],
    [7, 19],
    [3, 37],
    [1, 46],
    [32, 16],
    [28, 25],
    [30, 43],
    [34, 52],
    [23, 12],
    [21, 41],
    [50, 39],
    [48, 14],
];

/// Public re-export of EDGE_FACELETS — same rationale as CORNER_FACELETS_PUB.
pub const EDGE_FACELETS_PUB: [[usize; 2]; 12] = EDGE_FACELETS;

pub const FACE_MOVE_COUNT: usize = 18;
pub fn is_face_move(m: usize) -> bool {
    m < FACE_MOVE_COUNT
}

pub const NUM_CO_COORDS: usize = 2187; // 3^7
pub const NUM_EO_COORDS: usize = 2048; // 2^11
pub const NUM_SLICE_COORDS: usize = 495; // C(12, 4)
pub const NUM_CP_COORDS: usize = 40320; // 8!
pub const NUM_COORDS: usize = NUM_CO_COORDS * NUM_EO_COORDS;
pub const NUM_CO_SLICE: usize = NUM_CO_COORDS * NUM_SLICE_COORDS;
pub const NUM_EO_SLICE: usize = NUM_EO_COORDS * NUM_SLICE_COORDS;

pub const CANONICAL_SLICE: u32 = 494;

// ============================================================
// Encode / decode CO and EO.
// ============================================================

pub fn encode_co(orients: &[u8; 8]) -> u32 {
    let mut coord = 0u32;
    for i in 0..7 {
        coord = coord * 3 + orients[i] as u32;
    }
    coord
}

pub fn decode_co(coord: u32) -> [u8; 8] {
    let mut orients = [0u8; 8];
    let mut c = coord;
    let mut parity = 0u8;
    for i in (0..7).rev() {
        let o = (c % 3) as u8;
        c /= 3;
        orients[i] = o;
        parity = (parity + o) % 3;
    }
    orients[7] = (3 - parity) % 3;
    orients
}

pub fn encode_eo(orients: &[u8; 12]) -> u32 {
    let mut coord = 0u32;
    for i in 0..11 {
        coord = coord * 2 + orients[i] as u32;
    }
    coord
}

pub fn decode_eo(coord: u32) -> [u8; 12] {
    let mut orients = [0u8; 12];
    let mut c = coord;
    let mut parity = 0u8;
    for i in (0..11).rev() {
        let o = (c & 1) as u8;
        c >>= 1;
        orients[i] = o;
        parity ^= o;
    }
    orients[11] = parity;
    orients
}

// ============================================================
// Sticker → cubie identification.
// ============================================================

const CORNER_UD_COLOR: [i8; 8] = [
    Color::U, Color::U, Color::U, Color::U, Color::D, Color::D, Color::D, Color::D,
];

fn corner_lookup_key(a: i8, b: i8, c: i8) -> u32 {
    let mut arr = [a, b, c];
    arr.sort();
    (arr[0] as u32) * 36 + (arr[1] as u32) * 6 + (arr[2] as u32)
}

fn edge_lookup_key(a: i8, b: i8) -> u32 {
    if a <= b {
        (a as u32) * 6 + (b as u32)
    } else {
        (b as u32) * 6 + (a as u32)
    }
}

struct CornerLookup {
    // key -> cubie index (key fits in [0, 216))
    table: [i8; 216],
}

struct EdgeLookup {
    // key -> cubie index (key fits in [0, 36))
    table: [i8; 36],
}

static CORNER_LOOKUP: OnceLock<CornerLookup> = OnceLock::new();
static EDGE_LOOKUP: OnceLock<EdgeLookup> = OnceLock::new();

fn corner_lookup() -> &'static CornerLookup {
    CORNER_LOOKUP.get_or_init(|| {
        let mut t = [-1i8; 216];
        let corner_stickers: [[i8; 3]; 8] = [
            [Color::U, Color::R, Color::F],
            [Color::U, Color::F, Color::L],
            [Color::U, Color::L, Color::B],
            [Color::U, Color::B, Color::R],
            [Color::D, Color::F, Color::R],
            [Color::D, Color::L, Color::F],
            [Color::D, Color::B, Color::L],
            [Color::D, Color::R, Color::B],
        ];
        for i in 0..8 {
            let k = corner_lookup_key(
                corner_stickers[i][0],
                corner_stickers[i][1],
                corner_stickers[i][2],
            );
            t[k as usize] = i as i8;
        }
        CornerLookup { table: t }
    })
}

fn edge_lookup() -> &'static EdgeLookup {
    EDGE_LOOKUP.get_or_init(|| {
        let mut t = [-1i8; 36];
        let edge_stickers: [[i8; 2]; 12] = [
            [Color::U, Color::R],
            [Color::U, Color::F],
            [Color::U, Color::L],
            [Color::U, Color::B],
            [Color::D, Color::R],
            [Color::D, Color::F],
            [Color::D, Color::L],
            [Color::D, Color::B],
            [Color::F, Color::R],
            [Color::F, Color::L],
            [Color::B, Color::L],
            [Color::B, Color::R],
        ];
        for i in 0..12 {
            let k = edge_lookup_key(edge_stickers[i][0], edge_stickers[i][1]);
            t[k as usize] = i as i8;
        }
        EdgeLookup { table: t }
    })
}

fn lookup_corner(va: i8, vb: i8, vc: i8) -> Option<usize> {
    let key = corner_lookup_key(va, vb, vc);
    let id = corner_lookup().table[key as usize];
    if id < 0 {
        None
    } else {
        Some(id as usize)
    }
}

fn lookup_edge(va: i8, vb: i8) -> Option<usize> {
    let key = edge_lookup_key(va, vb);
    let id = edge_lookup().table[key as usize];
    if id < 0 {
        None
    } else {
        Some(id as usize)
    }
}

pub fn corner_ori_at(state: &[i8; FACELET_COUNT], position: usize) -> i32 {
    let [a, b, c] = CORNER_FACELETS[position];
    let va = state[a];
    let vb = state[b];
    let vc = state[c];
    if va < 0 || vb < 0 || vc < 0 {
        return -1;
    }
    let cubie = match lookup_corner(va, vb, vc) {
        Some(id) => id,
        None => return -1,
    };
    let ud = CORNER_UD_COLOR[cubie];
    if va == ud {
        0
    } else if vb == ud {
        1
    } else if vc == ud {
        2
    } else {
        -1
    }
}

pub fn edge_ori_at(state: &[i8; FACELET_COUNT], position: usize) -> i32 {
    let [a, b] = EDGE_FACELETS[position];
    let va = state[a];
    let vb = state[b];
    if va < 0 || vb < 0 {
        return -1;
    }
    let cubie = match lookup_edge(va, vb) {
        Some(id) => id,
        None => return -1,
    };
    let ref_color: i8 = if cubie < 4 {
        Color::U
    } else if cubie < 8 {
        Color::D
    } else if cubie < 10 {
        Color::F
    } else {
        Color::B
    };
    if va == ref_color {
        0
    } else {
        1
    }
}

pub fn encode_corner_ori(state: &[i8; FACELET_COUNT]) -> i32 {
    let mut coord = 0i32;
    for i in 0..7 {
        let o = corner_ori_at(state, i);
        if o < 0 {
            return -1;
        }
        coord = coord * 3 + o;
    }
    coord
}

pub fn encode_edge_ori(state: &[i8; FACELET_COUNT]) -> i32 {
    let mut coord = 0i32;
    for i in 0..11 {
        let o = edge_ori_at(state, i);
        if o < 0 {
            return -1;
        }
        coord = coord * 2 + o;
    }
    coord
}

pub fn encode_coord(state: &[i8; FACELET_COUNT]) -> i32 {
    let co = encode_corner_ori(state);
    let eo = encode_edge_ori(state);
    if co < 0 || eo < 0 {
        -1
    } else {
        co * (NUM_EO_COORDS as i32) + eo
    }
}

// ============================================================
// UD-slice coord.
// ============================================================

static BINOMIAL_C: OnceLock<[[u32; 5]; 13]> = OnceLock::new();

fn binomial_c() -> &'static [[u32; 5]; 13] {
    BINOMIAL_C.get_or_init(|| {
        let mut t = [[0u32; 5]; 13];
        for n in 0..13 {
            t[n][0] = 1;
            for k in 1..=4 {
                t[n][k] = if n > 0 { t[n - 1][k - 1] + t[n - 1][k] } else { 0 };
            }
        }
        t
    })
}

fn slice_cubie_at(state: &[i8; FACELET_COUNT], position: usize) -> i32 {
    let [a, b] = EDGE_FACELETS[position];
    let va = state[a];
    let vb = state[b];
    if va < 0 || vb < 0 {
        return -1;
    }
    match lookup_edge(va, vb) {
        Some(cubie) => {
            if cubie >= 8 {
                1
            } else {
                0
            }
        }
        None => -1,
    }
}

pub fn encode_slice(state: &[i8; FACELET_COUNT]) -> i32 {
    let mut coord = 0u32;
    let mut k = 0usize;
    for p in 0..12 {
        let s = slice_cubie_at(state, p);
        if s < 0 {
            return -1;
        }
        if s == 1 {
            k += 1;
            coord += binomial_c()[p][k];
        }
    }
    coord as i32
}

pub fn decode_slice(coord: u32) -> [bool; 12] {
    let mut occ = [false; 12];
    let mut c = coord;
    let mut k = 4usize;
    for p in (0..12).rev() {
        if k == 0 {
            break;
        }
        let b = binomial_c()[p][k];
        if c >= b {
            occ[p] = true;
            c -= b;
            if k > 0 {
                k -= 1;
            }
        }
    }
    occ
}

fn encode_slice_from_occ(occ: &[bool; 12]) -> u32 {
    let mut coord = 0u32;
    let mut k = 0usize;
    for p in 0..12 {
        if occ[p] {
            k += 1;
            coord += binomial_c()[p][k];
        }
    }
    coord
}

// ============================================================
// Option C — candidate enumeration over wildcarded states.
// ============================================================

fn could_be_ud(c: i8) -> bool {
    c == WILDCARD || c == Color::U || c == Color::D
}
fn could_be_fb(c: i8) -> bool {
    c == WILDCARD || c == Color::F || c == Color::B
}
fn could_be_rl(c: i8) -> bool {
    c == WILDCARD || c == Color::R || c == Color::L
}
fn could_be_side(c: i8) -> bool {
    c == WILDCARD || c == Color::R || c == Color::L || c == Color::F || c == Color::B
}

pub fn possible_corner_orients(state: &[i8; FACELET_COUNT], position: usize) -> Vec<u8> {
    let [fa, fb, fc] = CORNER_FACELETS[position];
    let va = state[fa];
    let vb = state[fb];
    let vc = state[fc];
    let mut out = Vec::with_capacity(3);
    if could_be_ud(va) && could_be_side(vb) && could_be_side(vc) {
        out.push(0);
    }
    if could_be_side(va) && could_be_ud(vb) && could_be_side(vc) {
        out.push(1);
    }
    if could_be_side(va) && could_be_side(vb) && could_be_ud(vc) {
        out.push(2);
    }
    out
}

pub fn possible_edge_orients(state: &[i8; FACELET_COUNT], position: usize) -> Vec<u8> {
    let [fa, fb] = EDGE_FACELETS[position];
    let va = state[fa];
    let vb = state[fb];
    let mut out = Vec::with_capacity(2);
    if (could_be_ud(va) && could_be_side(vb)) || (could_be_fb(va) && could_be_rl(vb)) {
        out.push(0);
    }
    if (could_be_side(va) && could_be_ud(vb)) || (could_be_rl(va) && could_be_fb(vb)) {
        out.push(1);
    }
    out
}

fn possible_slice_membership(state: &[i8; FACELET_COUNT], position: usize) -> (bool, bool) {
    let [fa, fb] = EDGE_FACELETS[position];
    let va = state[fa];
    let vb = state[fb];
    let ud = (could_be_ud(va) && could_be_side(vb)) || (could_be_side(va) && could_be_ud(vb));
    let sl = (could_be_fb(va) && could_be_rl(vb)) || (could_be_rl(va) && could_be_fb(vb));
    (ud, sl)
}

pub fn enumerate_corner_ori_coords(state: &[i8; FACELET_COUNT]) -> Vec<u32> {
    let mut possible: Vec<Vec<u8>> = Vec::with_capacity(8);
    for p in 0..8 {
        let list = possible_corner_orients(state, p);
        if list.is_empty() {
            return Vec::new();
        }
        possible.push(list);
    }
    let mut result = Vec::new();
    let mut orients = [0u8; 7];
    fn rec(
        i: usize,
        sum: u8,
        possible: &[Vec<u8>],
        orients: &mut [u8; 7],
        result: &mut Vec<u32>,
    ) {
        if i == 7 {
            let o7 = (3 - (sum % 3)) % 3;
            if possible[7].contains(&o7) {
                let mut coord = 0u32;
                for j in 0..7 {
                    coord = coord * 3 + orients[j] as u32;
                }
                result.push(coord);
            }
            return;
        }
        for &o in &possible[i] {
            orients[i] = o;
            rec(i + 1, sum + o, possible, orients, result);
        }
    }
    rec(0, 0, &possible, &mut orients, &mut result);
    result
}

pub fn enumerate_edge_ori_coords(state: &[i8; FACELET_COUNT]) -> Vec<u32> {
    let mut possible: Vec<Vec<u8>> = Vec::with_capacity(12);
    for p in 0..12 {
        let list = possible_edge_orients(state, p);
        if list.is_empty() {
            return Vec::new();
        }
        possible.push(list);
    }
    let mut result = Vec::new();
    let mut orients = [0u8; 11];
    fn rec(
        i: usize,
        parity: u8,
        possible: &[Vec<u8>],
        orients: &mut [u8; 11],
        result: &mut Vec<u32>,
    ) {
        if i == 11 {
            if possible[11].contains(&parity) {
                let mut coord = 0u32;
                for j in 0..11 {
                    coord = coord * 2 + orients[j] as u32;
                }
                result.push(coord);
            }
            return;
        }
        for &o in &possible[i] {
            orients[i] = o;
            rec(i + 1, parity ^ o, possible, orients, result);
        }
    }
    rec(0, 0, &possible, &mut orients, &mut result);
    result
}

pub fn enumerate_slice_coords(state: &[i8; FACELET_COUNT]) -> Vec<u32> {
    let mut mem = [(false, false); 12];
    for p in 0..12 {
        mem[p] = possible_slice_membership(state, p);
    }
    let mut result = Vec::new();
    let mut occ = [false; 12];
    fn rec(p: usize, count: usize, mem: &[(bool, bool); 12], occ: &mut [bool; 12], result: &mut Vec<u32>) {
        if count > 4 {
            return;
        }
        if 4 - count > 12 - p {
            return;
        }
        if p == 12 {
            if count == 4 {
                result.push(encode_slice_from_occ(occ));
            }
            return;
        }
        if mem[p].1 {
            occ[p] = true;
            rec(p + 1, count + 1, mem, occ, result);
            occ[p] = false;
        }
        if mem[p].0 {
            rec(p + 1, count, mem, occ, result);
        }
    }
    rec(0, 0, &mem, &mut occ, &mut result);
    result
}

pub fn has_wildcards(state: &[i8; FACELET_COUNT]) -> bool {
    state.iter().any(|&v| v == WILDCARD)
}

// ============================================================
// Corner permutation coord (factorial number system).
// ============================================================

pub const FACTORIAL: [u32; 9] = [1, 1, 2, 6, 24, 120, 720, 5040, 40320];

pub fn encode_corner_perm(perm: &[u8; 8]) -> u32 {
    let mut coord = 0u32;
    for i in 0..8 {
        let mut inv = 0u32;
        for j in (i + 1)..8 {
            if perm[j] < perm[i] {
                inv += 1;
            }
        }
        coord += inv * FACTORIAL[7 - i];
    }
    coord
}

pub fn decode_corner_perm(coord: u32) -> [u8; 8] {
    let mut perm = [0u8; 8];
    let mut avail: Vec<u8> = (0..8).collect();
    let mut c = coord;
    for i in 0..8 {
        let fac = FACTORIAL[7 - i];
        let k = (c / fac) as usize;
        c -= k as u32 * fac;
        perm[i] = avail.remove(k);
    }
    perm
}

pub fn encode_corner_perm_from_state(state: &[i8; FACELET_COUNT]) -> i32 {
    let mut perm = [0u8; 8];
    for p in 0..8 {
        let [a, b, c] = CORNER_FACELETS[p];
        if state[a] < 0 || state[b] < 0 || state[c] < 0 {
            return -1;
        }
        match lookup_corner(state[a], state[b], state[c]) {
            Some(id) => perm[p] = id as u8,
            None => return -1,
        }
    }
    encode_corner_perm(&perm) as i32
}

fn identify_corner_cubie_at(state: &[i8; FACELET_COUNT], position: usize) -> usize {
    let [a, b, c] = CORNER_FACELETS[position];
    lookup_corner(state[a], state[b], state[c]).expect("unknown corner colors")
}

fn identify_edge_cubie_at(state: &[i8; FACELET_COUNT], position: usize) -> usize {
    let [a, b] = EDGE_FACELETS[position];
    lookup_edge(state[a], state[b]).expect("unknown edge colors")
}

// ============================================================
// Move tables (per-coord transition arrays).
// ============================================================

pub struct MovePermData {
    pub corner_perm: Vec<[u8; 8]>,
    pub corner_ori_delta: Vec<[u8; 8]>,
    pub edge_perm: Vec<[u8; 12]>,
    pub edge_ori_delta: Vec<[u8; 12]>,
}

static MOVE_PERM_DATA: OnceLock<MovePermData> = OnceLock::new();

pub fn move_perm_data() -> &'static MovePermData {
    MOVE_PERM_DATA.get_or_init(|| {
        let mut cp_list = Vec::with_capacity(MOVE_COUNT);
        let mut cd_list = Vec::with_capacity(MOVE_COUNT);
        let mut ep_list = Vec::with_capacity(MOVE_COUNT);
        let mut ed_list = Vec::with_capacity(MOVE_COUNT);
        for m in 0..MOVE_COUNT {
            let after = apply_moves(&create_solved_facelet(), &[m]);
            let mut cp = [0u8; 8];
            let mut cd = [0u8; 8];
            for p in 0..8 {
                cp[p] = identify_corner_cubie_at(&after, p) as u8;
                cd[p] = corner_ori_at(&after, p) as u8;
            }
            let mut ep = [0u8; 12];
            let mut ed = [0u8; 12];
            for p in 0..12 {
                ep[p] = identify_edge_cubie_at(&after, p) as u8;
                ed[p] = edge_ori_at(&after, p) as u8;
            }
            cp_list.push(cp);
            cd_list.push(cd);
            ep_list.push(ep);
            ed_list.push(ed);
        }
        MovePermData {
            corner_perm: cp_list,
            corner_ori_delta: cd_list,
            edge_perm: ep_list,
            edge_ori_delta: ed_list,
        }
    })
}

pub struct MoveTables {
    pub co: Vec<u32>,
    pub eo: Vec<u32>,
    pub slice: Vec<u32>,
    pub cp: Vec<u32>,
}

static MOVE_TABLES: OnceLock<MoveTables> = OnceLock::new();

fn build_corner_move_table() -> Vec<u32> {
    let data = move_perm_data();
    let mut table = vec![0u32; MOVE_COUNT * NUM_CO_COORDS];
    for co in 0..NUM_CO_COORDS {
        let orients = decode_co(co as u32);
        for m in 0..MOVE_COUNT {
            let cp = &data.corner_perm[m];
            let cd = &data.corner_ori_delta[m];
            let mut new_orients = [0u8; 8];
            for p in 0..8 {
                new_orients[p] = (orients[cp[p] as usize] + cd[p]) % 3;
            }
            table[m * NUM_CO_COORDS + co] = encode_co(&new_orients);
        }
    }
    table
}

fn build_edge_move_table() -> Vec<u32> {
    let data = move_perm_data();
    let mut table = vec![0u32; MOVE_COUNT * NUM_EO_COORDS];
    for eo in 0..NUM_EO_COORDS {
        let orients = decode_eo(eo as u32);
        for m in 0..MOVE_COUNT {
            let ep = &data.edge_perm[m];
            let ed = &data.edge_ori_delta[m];
            let mut new_orients = [0u8; 12];
            for p in 0..12 {
                new_orients[p] = (orients[ep[p] as usize] + ed[p]) & 1;
            }
            table[m * NUM_EO_COORDS + eo] = encode_eo(&new_orients);
        }
    }
    table
}

fn build_slice_move_table() -> Vec<u32> {
    let data = move_perm_data();
    let mut table = vec![0u32; MOVE_COUNT * NUM_SLICE_COORDS];
    for slice in 0..NUM_SLICE_COORDS {
        let occ = decode_slice(slice as u32);
        for m in 0..MOVE_COUNT {
            let ep = &data.edge_perm[m];
            let mut new_occ = [false; 12];
            for p in 0..12 {
                new_occ[p] = occ[ep[p] as usize];
            }
            table[m * NUM_SLICE_COORDS + slice] = encode_slice_from_occ(&new_occ);
        }
    }
    table
}

fn build_corner_perm_move_table() -> Vec<u32> {
    let data = move_perm_data();
    let mut table = vec![0u32; MOVE_COUNT * NUM_CP_COORDS];
    for cp_coord in 0..NUM_CP_COORDS {
        let perm = decode_corner_perm(cp_coord as u32);
        for m in 0..MOVE_COUNT {
            let src = &data.corner_perm[m];
            let mut new_perm = [0u8; 8];
            for p in 0..8 {
                new_perm[p] = perm[src[p] as usize];
            }
            table[m * NUM_CP_COORDS + cp_coord] = encode_corner_perm(&new_perm);
        }
    }
    table
}

pub fn get_move_tables() -> &'static MoveTables {
    MOVE_TABLES.get_or_init(|| MoveTables {
        co: build_corner_move_table(),
        eo: build_edge_move_table(),
        slice: build_slice_move_table(),
        cp: build_corner_perm_move_table(),
    })
}

/// Apply a move to the (CO × EO) coord. For sanity tests.
pub fn apply_move_to_coord(coord: i32, m: usize) -> i32 {
    assert!(m < MOVE_COUNT, "applyMoveToCoord: move {} out of range", m);
    let tables = get_move_tables();
    let old_co = coord as u32 / NUM_EO_COORDS as u32;
    let old_eo = coord as u32 % NUM_EO_COORDS as u32;
    let new_co = tables.co[m * NUM_CO_COORDS + old_co as usize];
    let new_eo = tables.eo[m * NUM_EO_COORDS + old_eo as usize];
    (new_co * NUM_EO_COORDS as u32 + new_eo) as i32
}
