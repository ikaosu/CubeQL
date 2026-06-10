//! `#[wasm_bindgen]` API surface for the React renderer.
//!
//! Single source of truth for cube state / DSL / mask logic: the renderer
//! imports this module's exports (via `wasm-pack` output) instead of carrying
//! a duplicate TS engine. Search and pruning still live on the native side
//! and are invoked through Tauri IPC.

use serde::Serialize;
use wasm_bindgen::prelude::*;

use crate::dsl::format::{format_move_sequence as fmt_moves, format_pattern as fmt_pattern};
use crate::dsl::parser::parse_pattern as parse_pat;
use crate::facelet::{
    create_solved_facelet, validate_facelet as validate_facelet_native, FACELET_COUNT,
};
use crate::mask::{create_empty_mask, create_full_mask};
use crate::moves::{apply_moves as apply_moves_native, move_perm, parse_move_token as parse_tok};
use crate::output_mode::{output_mode_from_str, output_mode_to_str};
use crate::presets::{mask_from_preset_id, mask_from_string, mask_to_string, preset_by_id, presets};
use crate::project::{
    build_project, facelet_from_string, parse_project as parse_proj,
    project_to_facelet as proj_to_facelet, project_to_mask as proj_to_mask,
};

fn to_jsvalue<T: Serialize>(v: &T) -> JsValue {
    serde_wasm_bindgen::to_value(v).unwrap_or(JsValue::NULL)
}

fn err(msg: impl Into<String>) -> JsValue {
    JsValue::from_str(&msg.into())
}

// =========================================================================
// Facelet / mask basics
// =========================================================================

#[wasm_bindgen(js_name = createSolvedFacelet)]
pub fn wasm_create_solved_facelet() -> Box<[i8]> {
    create_solved_facelet().to_vec().into_boxed_slice()
}

#[wasm_bindgen(js_name = createFullMask)]
pub fn wasm_create_full_mask() -> Box<[u8]> {
    create_full_mask().to_vec().into_boxed_slice()
}

#[wasm_bindgen(js_name = createEmptyMask)]
pub fn wasm_create_empty_mask() -> Box<[u8]> {
    create_empty_mask().to_vec().into_boxed_slice()
}

#[wasm_bindgen(js_name = validateFacelet)]
pub fn wasm_validate_facelet(facelet: &[i8], mask: Option<Box<[u8]>>) -> Option<String> {
    if facelet.len() != FACELET_COUNT {
        return Some(format!("facelet length {} != 54", facelet.len()));
    }
    let mut state = [0i8; FACELET_COUNT];
    state.copy_from_slice(facelet);
    match mask {
        Some(m) => {
            if m.len() != FACELET_COUNT {
                return Some(format!("mask length {} != 54", m.len()));
            }
            let mut mm = [0u8; FACELET_COUNT];
            mm.copy_from_slice(&m);
            validate_facelet_native(&state, Some(&mm))
        }
        None => validate_facelet_native(&state, None),
    }
}

// =========================================================================
// Moves
// =========================================================================

#[wasm_bindgen(js_name = parseMoveToken)]
pub fn wasm_parse_move_token(token: &str) -> Option<usize> {
    parse_tok(token)
}

/// Apply a sequence of move indices to a 54-byte facelet. Returns a new
/// 54-byte buffer.
#[wasm_bindgen(js_name = applyMoves)]
pub fn wasm_apply_moves(facelet: &[i8], moves: &[i32]) -> Result<Box<[i8]>, JsValue> {
    if facelet.len() != FACELET_COUNT {
        return Err(err(format!("facelet length {} != 54", facelet.len())));
    }
    let mut state = [0i8; FACELET_COUNT];
    state.copy_from_slice(facelet);
    let m_usize: Vec<usize> = moves.iter().map(|&m| m as usize).collect();
    let out = apply_moves_native(&state, &m_usize);
    Ok(out.to_vec().into_boxed_slice())
}

/// Apply the SAME permutation sequence to a mask byte-array (so don't-care
/// stickers travel with their cubies). 54 bytes in / 54 bytes out.
#[wasm_bindgen(js_name = applyMovesToMask)]
pub fn wasm_apply_moves_to_mask(mask: &[u8], moves: &[i32]) -> Result<Box<[u8]>, JsValue> {
    if mask.len() != FACELET_COUNT {
        return Err(err(format!("mask length {} != 54", mask.len())));
    }
    let perms = move_perm();
    let mut cur = mask.to_vec();
    let mut buf = vec![0u8; FACELET_COUNT];
    for &m in moves {
        let mi = m as usize;
        if mi >= perms.len() {
            return Err(err(format!("move index {} out of range", mi)));
        }
        let p = &perms[mi];
        for i in 0..FACELET_COUNT {
            buf[i] = cur[p[i] as usize];
        }
        std::mem::swap(&mut cur, &mut buf);
    }
    Ok(cur.into_boxed_slice())
}

#[wasm_bindgen(js_name = formatMoveSequence)]
pub fn wasm_format_move_sequence(moves: &[i32]) -> String {
    let m_usize: Vec<usize> = moves.iter().map(|&m| m as usize).collect();
    fmt_moves(&m_usize)
}

// =========================================================================
// Presets
// =========================================================================

#[derive(Serialize)]
struct PresetWire {
    id: String,
    label: String,
    description: String,
}

#[wasm_bindgen(js_name = presetList)]
pub fn wasm_preset_list() -> JsValue {
    let list: Vec<PresetWire> = presets()
        .into_iter()
        .map(|p| PresetWire {
            id: p.id.to_string(),
            label: p.label.to_string(),
            description: p.description.to_string(),
        })
        .collect();
    to_jsvalue(&list)
}

#[wasm_bindgen(js_name = presetById)]
pub fn wasm_preset_by_id(id: &str) -> JsValue {
    match preset_by_id(id) {
        Some(p) => to_jsvalue(&PresetWire {
            id: p.id.to_string(),
            label: p.label.to_string(),
            description: p.description.to_string(),
        }),
        None => JsValue::NULL,
    }
}

#[wasm_bindgen(js_name = maskFromPresetId)]
pub fn wasm_mask_from_preset_id(id: &str) -> Option<Box<[u8]>> {
    mask_from_preset_id(id).map(|m| m.to_vec().into_boxed_slice())
}

// =========================================================================
// CubeQL DSL
// =========================================================================

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DslOk {
    ok: bool,
    preview: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DslErr {
    ok: bool,
    message: String,
    offset: usize,
    line: usize,
    column: usize,
}

/// Parse a CubeQL pattern and return either
///   `{ok: true,  preview: "<R, U>{1, 4}"}`
/// or
///   `{ok: false, message, offset, line, column}`.
///
/// The renderer only needs to know whether the pattern is valid and to show
/// a pretty-printed preview / error — the structured Pattern itself stays in
/// Rust and is re-parsed on the search request side.
#[wasm_bindgen(js_name = parsePatternPreview)]
pub fn wasm_parse_pattern_preview(src: &str) -> JsValue {
    match parse_pat(src) {
        Ok(p) => to_jsvalue(&DslOk {
            ok: true,
            preview: fmt_pattern(&p),
        }),
        Err(e) => to_jsvalue(&DslErr {
            ok: false,
            message: e.0.message,
            offset: e.0.offset,
            line: e.0.line,
            column: e.0.column,
        }),
    }
}

// =========================================================================
// Project (.gencube.json) (de)serialization
// =========================================================================

#[wasm_bindgen(js_name = buildProjectJson)]
#[allow(clippy::too_many_arguments)]
pub fn wasm_build_project_json(
    facelet: &[i8],
    mask: &[u8],
    preset: Option<String>,
    dsl: String,
    max_moves: usize,
    output_mode: &str,
    top_n: Option<usize>,
) -> Result<String, JsValue> {
    if facelet.len() != FACELET_COUNT {
        return Err(err("facelet length != 54"));
    }
    if mask.len() != FACELET_COUNT {
        return Err(err("mask length != 54"));
    }
    let mut f = [0i8; FACELET_COUNT];
    f.copy_from_slice(facelet);
    let mut m = [0u8; FACELET_COUNT];
    m.copy_from_slice(mask);
    let om = output_mode_from_str(output_mode).ok_or_else(|| err("bad outputMode"))?;
    let proj = build_project(&f, &m, preset, dsl, max_moves, om, top_n);
    serde_json::to_string(&proj).map_err(|e| err(e.to_string()))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProjectWire {
    facelet: Vec<i8>,
    mask: Vec<u8>,
    preset: Option<String>,
    dsl: String,
    max_moves: usize,
    output_mode: String,
    top_n: Option<usize>,
}

/// Parse a `.gencube.json` string and return the renderer-facing form
/// (decoded facelet + mask byte arrays, plus the search params).
#[wasm_bindgen(js_name = parseProjectJson)]
pub fn wasm_parse_project_json(json: &str) -> Result<JsValue, JsValue> {
    let proj = parse_proj(json).map_err(err)?;
    let facelet = proj_to_facelet(&proj);
    let mask = proj_to_mask(&proj);
    let wire = ProjectWire {
        facelet: facelet.to_vec(),
        mask: mask.to_vec(),
        preset: proj.preset,
        dsl: proj.dsl,
        max_moves: proj.search.max_moves,
        output_mode: output_mode_to_str(proj.search.output_mode).to_string(),
        top_n: proj.search.top_n,
    };
    Ok(to_jsvalue(&wire))
}

/// Sticker string ("URFDLB" letters → facelet ints) — used when loading raw
/// facelet snippets outside a full project.
#[wasm_bindgen(js_name = faceletFromString)]
pub fn wasm_facelet_from_string(s: &str) -> Box<[i8]> {
    facelet_from_string(s).to_vec().into_boxed_slice()
}

#[wasm_bindgen(js_name = maskToString)]
pub fn wasm_mask_to_string(mask: &[u8]) -> Result<String, JsValue> {
    if mask.len() != FACELET_COUNT {
        return Err(err("mask length != 54"));
    }
    let mut m = [0u8; FACELET_COUNT];
    m.copy_from_slice(mask);
    Ok(mask_to_string(&m))
}

#[wasm_bindgen(js_name = maskFromStringStr)]
pub fn wasm_mask_from_string(s: &str) -> Box<[u8]> {
    mask_from_string(s).to_vec().into_boxed_slice()
}
