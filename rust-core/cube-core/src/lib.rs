//! cube-core: Rust port of the gen-search engine.
//!
//! Targets two consumers from the same crate:
//!  - **native** (cube-cli, src-tauri) — full engine including pruning and
//!    search. Anything that does file I/O or relies on `std::time::Instant`
//!    lives behind `cfg(not(target_arch = "wasm32"))`.
//!  - **wasm32** (renderer / browser) — lightweight subset: facelet, moves,
//!    mask, dsl, presets, project (de)serialization, plus a `wasm` module
//!    exposing `#[wasm_bindgen]` entry points so the React UI never imports
//!    a duplicate TS engine.

pub mod cubie;
pub mod cubie_coord;
#[cfg(not(target_arch = "wasm32"))]
pub mod cubie_pdb;
#[cfg(not(target_arch = "wasm32"))]
pub mod cubie_search;
pub mod dsl;
pub mod facelet;
pub mod mask;
pub mod moves;
pub mod output_mode;
pub mod presets;
pub mod project;

#[cfg(not(target_arch = "wasm32"))]
pub mod coord;
#[cfg(not(target_arch = "wasm32"))]
pub mod pruning;
#[cfg(not(target_arch = "wasm32"))]
pub mod rotations;
#[cfg(not(target_arch = "wasm32"))]
pub mod search;
#[cfg(not(target_arch = "wasm32"))]
pub mod subgroup;

#[cfg(target_arch = "wasm32")]
pub mod wasm;

pub use facelet::{
    Color, CENTER_FACELETS, COLOR_NAMES, FACELET_COUNT, Facelet, clone_facelet, create_solved_facelet,
    facelets_equal, validate_facelet,
};
pub use mask::{
    EffectiveFacelet, Mask, WILDCARD, bake_in_mask, create_empty_mask, create_full_mask,
    default_goal, is_goal, mask_from_indices, toggle_mask,
};
pub use moves::{
    AXIS_OF_BASE, BASE_COUNT, BASE_NAMES, MOVE_COUNT, MOVE_PERM, apply_move, apply_move_into,
    apply_moves, inverse_move, move_axis, move_base, move_name, move_of, move_variant,
    parse_move_token,
};
