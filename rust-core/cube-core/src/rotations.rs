//! Cube rotation group helpers.
//!
//! The 24 whole-cube rotations form a group (isomorphic to the cube's
//! rotational symmetry group, S₄). For "physically solved == solved" goal
//! semantics we need to:
//!  1. test a facelet state against all 24 rotated-solved facelets, and
//!  2. seed the pruning BFS from all 24 rotated-solved coord values so the
//!     heuristic returns 0 at any rotated-solved state (admissibility).
//!
//! Both pieces depend on the same 24 permutations, computed once below.

use std::sync::OnceLock;

use crate::facelet::{create_solved_facelet, Facelet, FACELET_COUNT};
use crate::moves::{compose_perm, move_perm, Base};

const ROTATION_BASES: [usize; 9] = [
    Base::X * 3,
    Base::X * 3 + 1,
    Base::X * 3 + 2,
    Base::Y * 3,
    Base::Y * 3 + 1,
    Base::Y * 3 + 2,
    Base::Z * 3,
    Base::Z * 3 + 1,
    Base::Z * 3 + 2,
];

fn identity() -> [i8; FACELET_COUNT] {
    let mut p = [0i8; FACELET_COUNT];
    for i in 0..FACELET_COUNT {
        p[i] = i as i8;
    }
    p
}

/// Closure under composition of `{identity} ∪ {x, x', x2, y, y', y2, z, z', z2}`.
/// The result is exactly 24 distinct facelet permutations — the cube rotation
/// group.
fn compute_rotations() -> Vec<[i8; FACELET_COUNT]> {
    let perms = move_perm();
    let mut out: Vec<[i8; FACELET_COUNT]> = Vec::with_capacity(24);
    out.push(identity());
    loop {
        let prev_len = out.len();
        let snapshot = out.clone();
        for p in &snapshot {
            for &rm in &ROTATION_BASES {
                let r = &perms[rm];
                let composed = compose_perm(r, p);
                if !out.iter().any(|q| *q == composed) {
                    out.push(composed);
                }
            }
        }
        if out.len() == prev_len {
            break;
        }
    }
    debug_assert_eq!(out.len(), 24, "cube rotation group must have 24 elements");
    out
}

static ROTATION_PERMS: OnceLock<Vec<[i8; FACELET_COUNT]>> = OnceLock::new();

/// The 24 cube rotation permutations as facelet `[i8; 54]` arrays. Index 0 is
/// the identity; the rest are in BFS-expansion order (no semantic meaning,
/// just stable).
pub fn rotation_perms() -> &'static [[i8; FACELET_COUNT]] {
    ROTATION_PERMS.get_or_init(compute_rotations).as_slice()
}

static ROTATED_SOLVED: OnceLock<Vec<Facelet>> = OnceLock::new();

/// The 24 rotated-solved facelet states. A physically-solved cube held in any
/// orientation matches exactly one of these.
pub fn rotated_solved_facelets() -> &'static [Facelet] {
    ROTATED_SOLVED.get_or_init(|| {
        let solved = create_solved_facelet();
        rotation_perms()
            .iter()
            .map(|perm| {
                let mut out = [0i8; FACELET_COUNT];
                for i in 0..FACELET_COUNT {
                    out[i] = solved[perm[i] as usize];
                }
                out
            })
            .collect()
    })
}

/// Rotation-tolerant goal check: `true` iff `state` matches `goal` (or any of
/// its 23 sibling rotations) at every non-WILDCARD position. `goal` is the
/// canonical solved facelet in current usage but the function is generic.
///
/// Used at the search terminal node so a physically solved cube held in any
/// orientation is accepted as solved without forcing the search to spend an
/// extra rotation move to canonicalise the frame.
pub fn is_solved_rotation_tolerant(state: &[i8; FACELET_COUNT], goal: &Facelet) -> bool {
    let perms = rotation_perms();
    'rot: for perm in perms {
        for i in 0..FACELET_COUNT {
            let v = state[i];
            if v == crate::mask::WILDCARD {
                continue;
            }
            // Rotated goal: R(goal)[i] = goal[R[i]]
            let g = goal[perm[i] as usize];
            if v != g {
                continue 'rot;
            }
        }
        return true;
    }
    false
}
