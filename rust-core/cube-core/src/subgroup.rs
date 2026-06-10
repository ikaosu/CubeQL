//! Subgroup-invariant pruning. See `core/subgroup.ts` for the conceptual
//! model. Given a set of allowed move bases, the generated subgroup
//! preserves certain facelet positions and possibly CO/EO/slice/CP coords.

use std::sync::OnceLock;

use std::collections::HashSet;

use crate::coord::{
    encode_corner_perm_from_state, get_move_tables, move_perm_data, CANONICAL_SLICE,
    NUM_CO_COORDS, NUM_CP_COORDS, NUM_EO_COORDS, NUM_SLICE_COORDS,
};
use crate::facelet::{Facelet, FACELET_COUNT};
use crate::mask::EffectiveFacelet;
use crate::moves::{move_perm, BASE_COUNT};

/// True iff `move_table[m][goal_coord] == goal_coord` — i.e. the move keeps
/// the goal coord as a fixed point. If every move in a subgroup satisfies
/// this for a given axis, then from any non-goal state of that axis the
/// subgroup can never reach the goal (orbit of `goal_coord` is `{goal_coord}`
/// alone). That is exactly the condition the search engine needs to prune
/// "impossible under this move set" states.
fn move_preserves_goal(table: &[u32], m: usize, num_coords: usize, goal_coord: u32) -> bool {
    table[m * num_coords + goal_coord as usize] == goal_coord
}

pub const GOAL_CO: u32 = 0;
pub const GOAL_EO: u32 = 0;
pub const GOAL_SLICE: u32 = CANONICAL_SLICE;
pub const GOAL_CP: u32 = 0;

struct BasePreserves {
    co: [bool; BASE_COUNT],
    eo: [bool; BASE_COUNT],
    slice: [bool; BASE_COUNT],
    cp: [bool; BASE_COUNT],
}

static BASE_PRESERVES: OnceLock<BasePreserves> = OnceLock::new();

fn base_preserves() -> &'static BasePreserves {
    BASE_PRESERVES.get_or_init(|| {
        let mt = get_move_tables();
        let mut bp = BasePreserves {
            co: [true; BASE_COUNT],
            eo: [true; BASE_COUNT],
            slice: [true; BASE_COUNT],
            cp: [true; BASE_COUNT],
        };
        // Use the GOAL-only preservation check: the move set's orbit of the
        // goal coord is `{goal_coord}` iff every move maps goal_coord to
        // itself. That is the precise condition for `check_subgroup_invariants`
        // to be a sound impossibility check. The earlier whole-coord-space
        // identity check was strictly stronger and caused the invariants to
        // silently disable themselves for `<R, U>` etc.
        for b in 0..BASE_COUNT {
            for v in 0..3 {
                let m = b * 3 + v;
                if !move_preserves_goal(&mt.co, m, NUM_CO_COORDS, 0) {
                    bp.co[b] = false;
                }
                if !move_preserves_goal(&mt.eo, m, NUM_EO_COORDS, 0) {
                    bp.eo[b] = false;
                }
                if !move_preserves_goal(&mt.slice, m, NUM_SLICE_COORDS, CANONICAL_SLICE) {
                    bp.slice[b] = false;
                }
                if !move_preserves_goal(&mt.cp, m, NUM_CP_COORDS, 0) {
                    bp.cp[b] = false;
                }
            }
        }
        bp
    })
}

#[derive(Debug, Clone)]
pub struct SubgroupInvariants {
    pub facelet_positions: Vec<usize>,
    pub preserves_co: bool,
    pub preserves_eo: bool,
    pub preserves_slice: bool,
    pub preserves_cp: bool,
}

impl SubgroupInvariants {
    pub fn has_any(&self) -> bool {
        !self.facelet_positions.is_empty()
            || self.preserves_co
            || self.preserves_eo
            || self.preserves_slice
            || self.preserves_cp
    }
}

pub fn compute_subgroup_invariants(bases: &[usize]) -> SubgroupInvariants {
    let bp = base_preserves();
    let perm = move_perm();
    let mut fixed = [1u8; FACELET_COUNT];
    for &b in bases {
        for v in 0..3 {
            let p = &perm[b * 3 + v];
            for i in 0..FACELET_COUNT {
                if p[i] as usize != i {
                    fixed[i] = 0;
                }
            }
        }
    }
    let mut facelet_positions = Vec::new();
    for i in 0..FACELET_COUNT {
        if fixed[i] != 0 {
            facelet_positions.push(i);
        }
    }
    let mut p_co = true;
    let mut p_eo = true;
    let mut p_sl = true;
    let mut p_cp = true;
    for &b in bases {
        if !bp.co[b] {
            p_co = false;
        }
        if !bp.eo[b] {
            p_eo = false;
        }
        if !bp.slice[b] {
            p_sl = false;
        }
        if !bp.cp[b] {
            p_cp = false;
        }
    }
    SubgroupInvariants {
        facelet_positions,
        preserves_co: p_co,
        preserves_eo: p_eo,
        preserves_slice: p_sl,
        preserves_cp: p_cp,
    }
}

// ============================================================
// Piece-orbit invariants (Lv1 single-piece + Lv2 piece-pair).
//
// Given a basis set B, the subgroup <B> acts on cube state. From solved
// (every piece at its home slot), the orbit of any single piece is the set
// of slots reachable via B-sequences — call it Orbit_B(i) for piece i.
// Two-piece orbits give correlated constraints: for piece pair (i, j),
// Orbit_B(i, j) ⊆ Orbit_B(i) × Orbit_B(j) but in general strictly smaller.
//
// At search root, we identify the (slot_of_i, slot_of_j) for every piece
// pair from the scrambled state and check membership in Orbit_B(i, j).
// If ANY pair is outside its orbit, the scramble is not in <B>·solved →
// no solution exists; prune to zero nodes.
//
// Cost: storage ~34 KB per basis; root check ~190 lookups. Both negligible.
// Subsumed cases: the per-facelet `facelet_positions` check is a strict
// subset of Lv1 single-piece orbits.
// ============================================================

pub const CORNER_COUNT: usize = 8;
pub const EDGE_COUNT: usize = 12;
const EDGE_PAIR_BITS: usize = EDGE_COUNT * EDGE_COUNT; // 144
const CROSS_PAIR_BITS: usize = CORNER_COUNT * EDGE_COUNT; // 96

/// `bits[a * dim + b]` is set iff (piece i at slot a, piece j at slot b) is
/// reachable from solved by the basis. Diagonal (a==b for i==j) is the Lv1
/// "single-piece in slot a" bit.
#[derive(Clone)]
pub struct PieceOrbits {
    /// Flattened [piece_i * 8 + piece_j] → 64-bit bitmask over (slot_i*8+slot_j).
    pub corner_pair: Vec<u64>,
    /// Flattened [piece_i * 12 + piece_j] → 144-bit bitmask (Vec<bool> 144).
    pub edge_pair: Vec<Vec<bool>>,
    /// Flattened [corner_piece * 12 + edge_piece] → 96-bit bitmask (Vec<bool> 96).
    pub cross_pair: Vec<Vec<bool>>,
    /// True iff the basis covers all 6 axes (R, L, U, D, F, B) — in that case
    /// every piece pair is fully reachable and the orbits are trivially the
    /// full grid; we skip computation and skip the root check.
    pub is_full_group: bool,
}

impl PieceOrbits {
    pub fn empty() -> Self {
        Self {
            corner_pair: Vec::new(),
            edge_pair: Vec::new(),
            cross_pair: Vec::new(),
            is_full_group: true,
        }
    }
}

/// Derive the per-move slot permutation σ such that the piece sitting at
/// slot s before the move ends up at slot σ(s). From `corner_perm[m]`
/// (= piece at slot s after move m, starting from solved): the piece that
/// is now at slot s came from slot corner_perm[m][s], so σ(corner_perm[m][s]) = s.
fn slot_perm_corner(m: usize) -> [u8; CORNER_COUNT] {
    let cp = &move_perm_data().corner_perm[m];
    let mut sp = [0u8; CORNER_COUNT];
    for s in 0..CORNER_COUNT {
        sp[cp[s] as usize] = s as u8;
    }
    sp
}

fn slot_perm_edge(m: usize) -> [u8; EDGE_COUNT] {
    let ep = &move_perm_data().edge_perm[m];
    let mut sp = [0u8; EDGE_COUNT];
    for s in 0..EDGE_COUNT {
        sp[ep[s] as usize] = s as u8;
    }
    sp
}

fn collect_basis_slot_perms(
    bases: &[usize],
) -> (Vec<[u8; CORNER_COUNT]>, Vec<[u8; EDGE_COUNT]>) {
    let mut corner_perms = Vec::new();
    let mut edge_perms = Vec::new();
    for &b in bases {
        // Only face-turn bases (R, L, U, D, F, B = 0..6) define corner/edge
        // perms; whole-cube rotations (x, y, z = 6..9) leave the abstract
        // piece arrangement intact but reframe slot labels — we treat them as
        // identity here, which is sound for the from-solved orbit calculation
        // since rotations don't change which piece is at which physical slot.
        if b >= 6 {
            continue;
        }
        for v in 0..3 {
            let m = b * 3 + v;
            corner_perms.push(slot_perm_corner(m));
            edge_perms.push(slot_perm_edge(m));
        }
    }
    (corner_perms, edge_perms)
}

/// Compute piece-orbit invariants for the given basis set via small BFS.
/// Falls back to `empty()` (trivially-true) when bases cover all 6 face axes.
pub fn compute_piece_orbits(bases: &[usize]) -> PieceOrbits {
    // Detect "full face group" — all 6 face axes present.
    let face_axes_present: std::collections::BTreeSet<usize> =
        bases.iter().copied().filter(|&b| b < 6).collect();
    if face_axes_present.len() == 6 {
        return PieceOrbits::empty();
    }
    // If the pattern uses any non-face-axis move (wide turns Uw/Rw/…/Bw or
    // slice moves M/E/S, indices 6..15), the abstract piece-orbit BFS would
    // under-estimate reachability — `collect_basis_slot_perms` skips those
    // bases because the cube-core doesn't have slot-perm tables for them, so
    // the orbit collapses to the face-only subgroup. Skip the fast-reject
    // (return the trivially-empty/full sentinel) and let actual DFS decide.
    // Pure whole-cube rotations (x/y/z, indices 15..18) really are identity
    // on the abstract piece arrangement so they stay safe to ignore.
    if bases.iter().any(|&b| (6..15).contains(&b)) {
        return PieceOrbits::empty();
    }

    let (corner_perms, edge_perms) = collect_basis_slot_perms(bases);

    // Corner pair BFS: 64 bits per pair, 64 pairs (including diagonal).
    let mut corner_pair = vec![0u64; CORNER_COUNT * CORNER_COUNT];
    for i in 0..CORNER_COUNT {
        for j in 0..CORNER_COUNT {
            let mut visited = 0u64;
            visited |= 1u64 << (i * CORNER_COUNT + j);
            let mut queue: Vec<(usize, usize)> = vec![(i, j)];
            while let Some((a, b)) = queue.pop() {
                for sp in &corner_perms {
                    let na = sp[a] as usize;
                    let nb = sp[b] as usize;
                    let bit = na * CORNER_COUNT + nb;
                    if visited & (1u64 << bit) == 0 {
                        visited |= 1u64 << bit;
                        queue.push((na, nb));
                    }
                }
            }
            corner_pair[i * CORNER_COUNT + j] = visited;
        }
    }

    // Edge pair BFS: 144 bits per pair.
    let mut edge_pair: Vec<Vec<bool>> = vec![vec![false; EDGE_PAIR_BITS]; EDGE_COUNT * EDGE_COUNT];
    for i in 0..EDGE_COUNT {
        for j in 0..EDGE_COUNT {
            let bits = &mut edge_pair[i * EDGE_COUNT + j];
            bits[i * EDGE_COUNT + j] = true;
            let mut queue: Vec<(usize, usize)> = vec![(i, j)];
            while let Some((a, b)) = queue.pop() {
                for sp in &edge_perms {
                    let na = sp[a] as usize;
                    let nb = sp[b] as usize;
                    let bit = na * EDGE_COUNT + nb;
                    if !bits[bit] {
                        bits[bit] = true;
                        queue.push((na, nb));
                    }
                }
            }
        }
    }

    // Cross pair BFS: 96 bits per pair, 96 pairs. State = (corner_slot, edge_slot).
    let mut cross_pair: Vec<Vec<bool>> =
        vec![vec![false; CROSS_PAIR_BITS]; CORNER_COUNT * EDGE_COUNT];
    for ci in 0..CORNER_COUNT {
        for ej in 0..EDGE_COUNT {
            let bits = &mut cross_pair[ci * EDGE_COUNT + ej];
            bits[ci * EDGE_COUNT + ej] = true;
            let mut queue: Vec<(usize, usize)> = vec![(ci, ej)];
            // Move count differs between corner and edge slot_perm Vecs only if
            // some basis contributed differently; in our construction they're
            // aligned 1:1, so use index.
            let n_moves = corner_perms.len();
            while let Some((cs, es)) = queue.pop() {
                for k in 0..n_moves {
                    let ncs = corner_perms[k][cs] as usize;
                    let nes = edge_perms[k][es] as usize;
                    let bit = ncs * EDGE_COUNT + nes;
                    if !bits[bit] {
                        bits[bit] = true;
                        queue.push((ncs, nes));
                    }
                }
            }
        }
    }

    PieceOrbits {
        corner_pair,
        edge_pair,
        cross_pair,
        is_full_group: false,
    }
}

/// Identify which corner piece is at each slot. Returns `None` for any slot
/// that has WILDCARD on any of its 3 facelets (mask present → can't tell).
fn identify_corner_perm_partial(state: &EffectiveFacelet) -> [Option<u8>; CORNER_COUNT] {
    use crate::coord::CORNER_FACELETS_PUB as CF;
    let mut out: [Option<u8>; CORNER_COUNT] = [None; CORNER_COUNT];
    let goal = crate::facelet::create_solved_facelet();
    for slot in 0..CORNER_COUNT {
        let [a, b, c] = CF[slot];
        if state[a] < 0 || state[b] < 0 || state[c] < 0 {
            continue;
        }
        // Find which corner home slot has matching color triple in solved.
        for piece in 0..CORNER_COUNT {
            let [pa, pb, pc] = CF[piece];
            let ga = goal[pa];
            let gb = goal[pb];
            let gc = goal[pc];
            // Compare as multiset (ignore facelet ordering, since orientation
            // may differ — we just want to identify the cubie).
            let cur = [state[a], state[b], state[c]];
            let target = [ga, gb, gc];
            if multiset_eq(&cur, &target) {
                out[slot] = Some(piece as u8);
                break;
            }
        }
    }
    out
}

fn identify_edge_perm_partial(state: &EffectiveFacelet) -> [Option<u8>; EDGE_COUNT] {
    use crate::coord::EDGE_FACELETS_PUB as EF;
    let mut out: [Option<u8>; EDGE_COUNT] = [None; EDGE_COUNT];
    let goal = crate::facelet::create_solved_facelet();
    for slot in 0..EDGE_COUNT {
        let [a, b] = EF[slot];
        if state[a] < 0 || state[b] < 0 {
            continue;
        }
        for piece in 0..EDGE_COUNT {
            let [pa, pb] = EF[piece];
            let ga = goal[pa];
            let gb = goal[pb];
            let cur = [state[a], state[b]];
            let target = [ga, gb];
            if multiset_eq(&cur, &target) {
                out[slot] = Some(piece as u8);
                break;
            }
        }
    }
    out
}

fn multiset_eq<T: Copy + Ord>(a: &[T], b: &[T]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut a: Vec<T> = a.to_vec();
    let mut b: Vec<T> = b.to_vec();
    a.sort();
    b.sort();
    a == b
}

/// Returns `true` iff the state is consistent with the basis's piece-pair
/// orbits — i.e., it could be in <basis>·solved. Returns `true` on full group
/// (no useful constraints). Pieces whose home slots have WILDCARD facelets
/// are skipped.
pub fn check_piece_orbits_at_state(state: &EffectiveFacelet, orbits: &PieceOrbits) -> bool {
    if orbits.is_full_group {
        return true;
    }

    let corner_at = identify_corner_perm_partial(state);
    let edge_at = identify_edge_perm_partial(state);

    // Build inverse maps: piece -> slot (None if not located in state).
    let mut corner_slot_of: [Option<u8>; CORNER_COUNT] = [None; CORNER_COUNT];
    for slot in 0..CORNER_COUNT {
        if let Some(piece) = corner_at[slot] {
            corner_slot_of[piece as usize] = Some(slot as u8);
        }
    }
    let mut edge_slot_of: [Option<u8>; EDGE_COUNT] = [None; EDGE_COUNT];
    for slot in 0..EDGE_COUNT {
        if let Some(piece) = edge_at[slot] {
            edge_slot_of[piece as usize] = Some(slot as u8);
        }
    }

    // Corner pairs.
    for i in 0..CORNER_COUNT {
        let Some(si) = corner_slot_of[i] else { continue };
        for j in 0..CORNER_COUNT {
            let Some(sj) = corner_slot_of[j] else { continue };
            let mask = orbits.corner_pair[i * CORNER_COUNT + j];
            let bit = (si as usize) * CORNER_COUNT + (sj as usize);
            if mask & (1u64 << bit) == 0 {
                return false;
            }
        }
    }

    // Edge pairs.
    for i in 0..EDGE_COUNT {
        let Some(si) = edge_slot_of[i] else { continue };
        for j in 0..EDGE_COUNT {
            let Some(sj) = edge_slot_of[j] else { continue };
            let bits = &orbits.edge_pair[i * EDGE_COUNT + j];
            let bit = (si as usize) * EDGE_COUNT + (sj as usize);
            if !bits[bit] {
                return false;
            }
        }
    }

    // Cross pairs (corner × edge).
    for ci in 0..CORNER_COUNT {
        let Some(cs) = corner_slot_of[ci] else { continue };
        for ej in 0..EDGE_COUNT {
            let Some(es) = edge_slot_of[ej] else { continue };
            let bits = &orbits.cross_pair[ci * EDGE_COUNT + ej];
            let bit = (cs as usize) * EDGE_COUNT + (es as usize);
            if !bits[bit] {
                return false;
            }
        }
    }

    true
}

/// Convenience wrapper that takes a canonical `Facelet` (no mask) — used by
/// tests and the goal-side caller.
pub fn check_piece_orbits_facelet(state: &Facelet, orbits: &PieceOrbits) -> bool {
    let mut ef: EffectiveFacelet = [0i8; FACELET_COUNT];
    for i in 0..FACELET_COUNT {
        ef[i] = state[i];
    }
    check_piece_orbits_at_state(&ef, orbits)
}

// Suppress unused-import warning if encode_corner_perm_from_state ever ceases
// to be used inside helpers below.
#[allow(dead_code)]
fn _keep_encode_used(state: &Facelet) -> i32 {
    encode_corner_perm_from_state(state)
}

// ============================================================
// Corner-state orbit BFS — strictly stronger than Lv1/Lv2 pair checks.
//
// Lv1+Lv2 (single-piece + pairwise) miss 3-piece correlations like a
// pure (URF UFL UBR) 3-cycle: each pair (URF, UFL), (URF, UBR), (UFL, UBR)
// is individually reachable in <R, U>·solved, but the simultaneous triple
// requires an algorithm that would also change CO — so the joint state
// (CP=non-trivial, CO=0) is not in the <R, U>-corner-orbit.
//
// BFS over (CP coord, CO coord) under the basis's face moves gives the
// exact reachable set in O(orbit_size · |basis|) time and O(orbit_size)
// memory. For <R, U>: 29,160 states → ~230 KB hashset. For full face group
// (URDLFB): orbit is the full corner group (~88M) — we skip in that case.
//
// Membership test at search root is one HashSet lookup. Catches the user's
// 3-cycle scramble and many other corner-only impossibilities.
// ============================================================

#[derive(Clone)]
pub struct CornerOrbit {
    pub set: HashSet<(u32, u32)>,
    /// True iff basis is the full 6-axis face group — orbit is the full corner
    /// group; we skip computation and the check is a no-op (always passes).
    pub is_full_group: bool,
}

impl CornerOrbit {
    pub fn empty() -> Self {
        Self {
            set: HashSet::new(),
            is_full_group: true,
        }
    }
}

/// BFS the (CP, CO) coord space under the basis's face moves. Skips and
/// returns `empty()` when the basis covers all 6 face axes.
pub fn compute_corner_orbit(bases: &[usize]) -> CornerOrbit {
    let face_axes_present: std::collections::BTreeSet<usize> =
        bases.iter().copied().filter(|&b| b < 6).collect();
    if face_axes_present.len() == 6 {
        return CornerOrbit::empty();
    }
    // Wide turn / slice move bases (6..15) don't have cube-core move tables
    // for the (CP, CO) space, so the orbit computed below would under-cover
    // reachable coords and falsely reject states reached via Uw/Rw/M/E/S.
    // Skip the fast-reject — DFS handles correctness via legacy facelet path.
    if bases.iter().any(|&b| (6..15).contains(&b)) {
        return CornerOrbit::empty();
    }

    let mt = get_move_tables();
    let mut moves: Vec<usize> = Vec::new();
    for &b in &face_axes_present {
        for v in 0..3 {
            moves.push(b * 3 + v);
        }
    }

    let mut set: HashSet<(u32, u32)> = HashSet::with_capacity(1 << 15);
    let mut queue: Vec<(u32, u32)> = vec![(0, 0)];
    set.insert((0, 0));
    while let Some((cp, co)) = queue.pop() {
        for &m in &moves {
            let nc = mt.cp[m * NUM_CP_COORDS + cp as usize];
            let no = mt.co[m * NUM_CO_COORDS + co as usize];
            let next = (nc, no);
            if set.insert(next) {
                queue.push(next);
            }
        }
    }
    CornerOrbit {
        set,
        is_full_group: false,
    }
}

/// Returns `true` iff the state's corner state (CP, CO) is in the basis's
/// orbit of (0, 0). Returns `true` for full group or for states with
/// undeterminable corner coords (any corner facelet is WILDCARD).
pub fn check_corner_orbit_at_state(state: &EffectiveFacelet, orbit: &CornerOrbit) -> bool {
    if orbit.is_full_group {
        return true;
    }
    let cp = encode_corner_perm_from_state(state);
    if cp < 0 {
        // Mask present on a corner facelet — can't determine; skip check.
        return true;
    }
    let co = crate::coord::encode_corner_ori(state);
    if co < 0 {
        return true;
    }
    orbit.set.contains(&(cp as u32, co as u32))
}
