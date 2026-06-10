//! Project file format (.gencube.json) — binary-compatible with the TS schema.

use serde::{Deserialize, Serialize};

use crate::facelet::{Facelet, COLOR_NAMES, FACELET_COUNT};
use crate::mask::Mask;
use crate::presets::{mask_from_string, mask_to_string};
use crate::output_mode::OutputMode;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectFile {
    pub version: u32,
    /// 54-character string of colors {U, R, F, D, L, B}.
    pub facelet: String,
    /// 54-character string of {0, 1}.
    pub mask: String,
    /// Preset id this was derived from, or null if custom.
    pub preset: Option<String>,
    pub dsl: String,
    pub search: ProjectSearch,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectSearch {
    #[serde(rename = "maxMoves")]
    pub max_moves: usize,
    #[serde(rename = "outputMode")]
    pub output_mode: OutputMode,
    #[serde(rename = "topN", skip_serializing_if = "Option::is_none")]
    pub top_n: Option<usize>,
}

pub fn facelet_to_string(f: &Facelet) -> String {
    let mut s = String::with_capacity(FACELET_COUNT);
    for i in 0..FACELET_COUNT {
        s.push_str(COLOR_NAMES[f[i] as usize]);
    }
    s
}

pub fn facelet_from_string(s: &str) -> Facelet {
    let mut out = [0i8; FACELET_COUNT];
    for (i, ch) in s.chars().enumerate() {
        if i >= FACELET_COUNT {
            break;
        }
        let token = ch.to_string();
        let idx = COLOR_NAMES.iter().position(|n| *n == token).unwrap_or(0);
        out[i] = idx as i8;
    }
    out
}

pub fn build_project(
    facelet: &Facelet,
    mask: &Mask,
    preset: Option<String>,
    dsl: String,
    max_moves: usize,
    output_mode: OutputMode,
    top_n: Option<usize>,
) -> ProjectFile {
    ProjectFile {
        version: 1,
        facelet: facelet_to_string(facelet),
        mask: mask_to_string(mask),
        preset,
        dsl,
        search: ProjectSearch {
            max_moves,
            output_mode,
            top_n,
        },
    }
}

pub fn parse_project(json: &str) -> Result<ProjectFile, String> {
    let p: ProjectFile = serde_json::from_str(json).map_err(|e| format!("parse project: {}", e))?;
    if p.version != 1 {
        return Err(format!("unsupported project version: {}", p.version));
    }
    if p.facelet.len() < FACELET_COUNT {
        return Err("invalid facelet field".to_string());
    }
    if p.mask.len() < FACELET_COUNT {
        return Err("invalid mask field".to_string());
    }
    Ok(p)
}

pub fn project_to_facelet(p: &ProjectFile) -> Facelet {
    facelet_from_string(&p.facelet)
}

pub fn project_to_mask(p: &ProjectFile) -> Mask {
    mask_from_string(&p.mask)
}
