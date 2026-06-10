//! Sticker masks: "traveling wildcard" semantics.
//!
//! See `core/mask.ts` for the conceptual model. The mask is applied to the
//! initial state ONCE via `bake_in_mask`, producing an `EffectiveFacelet`
//! whose wildcard positions hold `WILDCARD` (-1) and travel with permutations
//! during move application.

use crate::facelet::{create_solved_facelet, Facelet, FACELET_COUNT};

pub const WILDCARD: i8 = -1;

pub type Mask = [u8; FACELET_COUNT];
pub type EffectiveFacelet = [i8; FACELET_COUNT];

pub fn create_empty_mask() -> Mask {
    [0u8; FACELET_COUNT]
}

pub fn create_full_mask() -> Mask {
    [1u8; FACELET_COUNT]
}

pub fn mask_from_indices(indices: &[usize]) -> Mask {
    let mut m = [0u8; FACELET_COUNT];
    for &i in indices {
        m[i] = 1;
    }
    m
}

pub fn toggle_mask(mask: &mut Mask, index: usize) {
    mask[index] = if mask[index] != 0 { 0 } else { 1 };
}

pub fn bake_in_mask(initial: &Facelet, mask: &Mask) -> EffectiveFacelet {
    let mut out = [0i8; FACELET_COUNT];
    for i in 0..FACELET_COUNT {
        out[i] = if mask[i] != 0 { initial[i] } else { WILDCARD };
    }
    out
}

pub fn is_goal(state: &EffectiveFacelet, goal: &Facelet) -> bool {
    for i in 0..FACELET_COUNT {
        let v = state[i];
        if v != WILDCARD && v != goal[i] {
            return false;
        }
    }
    true
}

pub fn default_goal() -> Facelet {
    create_solved_facelet()
}
