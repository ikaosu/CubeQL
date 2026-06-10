//! Preset masks for common last-layer / first-2-layers solves.

use crate::facelet::FACELET_COUNT;
use crate::mask::{mask_from_indices, Mask};

// Region constants.
pub const U_FACE: &[usize] = &[0, 1, 2, 3, 4, 5, 6, 7, 8];
pub const U_FACE_CORNERS: &[usize] = &[0, 2, 6, 8];
pub const U_FACE_EDGES: &[usize] = &[1, 3, 5, 7];
pub const U_FACE_CENTER: &[usize] = &[4];

pub const U_LAYER_SIDE: &[usize] = &[9, 10, 11, 18, 19, 20, 36, 37, 38, 45, 46, 47];
pub const U_LAYER_SIDE_CORNERS: &[usize] = &[9, 11, 18, 20, 36, 38, 45, 47];
pub const U_LAYER_SIDE_EDGES: &[usize] = &[10, 19, 37, 46];

pub const E_SLICE: &[usize] = &[12, 13, 14, 21, 22, 23, 39, 40, 41, 48, 49, 50];
pub const D_LAYER_SIDE: &[usize] = &[15, 16, 17, 24, 25, 26, 42, 43, 44, 51, 52, 53];
pub const D_FACE: &[usize] = &[27, 28, 29, 30, 31, 32, 33, 34, 35];

// Lower-2-layers: D face + D-side + E-slice = 33 stickers
pub const LOWER_TWO_LAYERS: &[usize] = &[
    27, 28, 29, 30, 31, 32, 33, 34, 35, // D face
    15, 16, 17, 24, 25, 26, 42, 43, 44, 51, 52, 53, // D-layer side
    12, 13, 14, 21, 22, 23, 39, 40, 41, 48, 49, 50, // E-slice
];

pub const D_FACE_F2B: &[usize] = &[27, 29, 30, 32, 33, 35]; // D-face minus DF / D-center / DB
pub const D_LAYER_SIDE_F2B: &[usize] = &[15, 16, 17, 24, 26, 42, 43, 44, 51, 53];
/// Roux's left + right 1×2×3 blocks — everything except the M-slice and the
/// entire U-layer. M-slice stickers (U/D/F/B centers and middle-column edges)
/// are NOT fixed by F2B because M turns leave the blocks intact.
pub const ROUX_F2B: &[usize] = &[
    // D face block stickers (excludes 28 = DF, 31 = D center, 34 = DB).
    27, 29, 30, 32, 33, 35,
    // R-face block (middle + bottom rows excluding U-layer top row).
    12, 13, 14, 15, 16, 17,
    // L-face block (middle + bottom rows).
    39, 40, 41, 42, 43, 44,
    // F-face block stickers from the L/R blocks (FL and FR pillars, excluding
    // F-center column 19/22/25).
    21, 23, 24, 26,
    // B-face block stickers (mirror of F).
    48, 50, 51, 53,
];


#[derive(Debug, Clone)]
pub struct Preset {
    pub id: &'static str,
    pub label: &'static str,
    pub visible_indices: Vec<usize>,
    pub description: &'static str,
}

fn concat(parts: &[&[usize]]) -> Vec<usize> {
    let mut out: Vec<usize> = Vec::new();
    for p in parts {
        out.extend_from_slice(p);
    }
    out
}

pub fn presets() -> Vec<Preset> {
    vec![
        Preset {
            id: "OLL",
            label: "OLL — Orient Last Layer",
            visible_indices: concat(&[U_FACE, LOWER_TWO_LAYERS]),
            description:
                "U face oriented (uniform) + lower 2 layers solved. U-side don't-care. Prepend/append <U> to your DSL if you need final AUF freedom.",
        },
        Preset {
            id: "PLL",
            label: "PLL — Permute Last Layer",
            visible_indices: concat(&[U_FACE, U_LAYER_SIDE, LOWER_TWO_LAYERS]),
            description: "Entire cube must be solved. Prepend/append <U> to your DSL for AUF tolerance.",
        },
        Preset {
            id: "F2L",
            label: "F2L — First 2 Layers",
            visible_indices: LOWER_TWO_LAYERS.to_vec(),
            description: "Lower 2 layers solved (D face + D-side + E-slice). U layer unconstrained.",
        },
        Preset {
            id: "CMLL",
            label: "CMLL — Corners of Last Layer (Roux)",
            // U / D / F / B centers are deliberately don't-care so the cube
            // can be held in any orientation around the L-R axis. R / L
            // centers stay visible (they're part of the Roux F2B blocks).
            visible_indices: concat(&[
                U_FACE_CORNERS,
                U_LAYER_SIDE_CORNERS,
                ROUX_F2B,
            ]),
            description:
                "Roux F2B + U-layer corners solved. U/D/F/B centers are don't-care so the cube orientation around the L-R axis is free. DF/DB and all U-layer edges remain free (LSE pending).",
        },
        Preset {
            id: "COLL",
            label: "COLL — Corner Orient + Perm (Last Layer)",
            visible_indices: concat(&[U_FACE, U_LAYER_SIDE_CORNERS, LOWER_TWO_LAYERS]),
            description:
                "Same visible region as CMLL (U face + U-layer corner side stickers + lower 2 layers).",
        },
        Preset {
            id: "EOLL",
            label: "EOLL — Edges of Last Layer (orientation only)",
            visible_indices: concat(&[U_FACE_EDGES, LOWER_TWO_LAYERS]),
            description: "U-face edge stickers must be U color (orient last-layer edges). Corners ignored.",
        },
        Preset {
            id: "OCLL",
            label: "OCLL — Corners of Last Layer (orientation only)",
            visible_indices: concat(&[U_FACE_CORNERS, LOWER_TWO_LAYERS]),
            description: "U-face corner stickers must be U color. U-face edges ignored.",
        },
    ]
}

pub fn preset_by_id(id: &str) -> Option<Preset> {
    presets().into_iter().find(|p| p.id == id)
}

pub fn mask_from_preset_id(id: &str) -> Option<Mask> {
    Some(mask_from_indices(&preset_by_id(id)?.visible_indices))
}

pub fn mask_to_string(mask: &Mask) -> String {
    let mut s = String::with_capacity(FACELET_COUNT);
    for i in 0..FACELET_COUNT {
        s.push(if mask[i] != 0 { '1' } else { '0' });
    }
    s
}

pub fn mask_from_string(s: &str) -> Mask {
    let mut m = [0u8; FACELET_COUNT];
    for (i, ch) in s.chars().enumerate() {
        if i >= FACELET_COUNT {
            break;
        }
        m[i] = if ch == '1' { 1 } else { 0 };
    }
    m
}
