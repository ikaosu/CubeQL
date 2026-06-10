//! `OutputMode` lives in its own module so the WASM build (which excludes the
//! search engine) can still reference it from `project.rs` and the renderer
//! side via the WASM bridge.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OutputMode {
    #[serde(rename = "enumerate-up-to-N")]
    EnumerateUpToN,
    #[serde(rename = "shortest-only")]
    ShortestOnly,
    #[serde(rename = "top-n")]
    TopN,
}

pub fn output_mode_from_str(s: &str) -> Option<OutputMode> {
    match s {
        "shortest-only" => Some(OutputMode::ShortestOnly),
        "enumerate-up-to-N" => Some(OutputMode::EnumerateUpToN),
        "top-n" => Some(OutputMode::TopN),
        _ => None,
    }
}

pub fn output_mode_to_str(m: OutputMode) -> &'static str {
    match m {
        OutputMode::ShortestOnly => "shortest-only",
        OutputMode::EnumerateUpToN => "enumerate-up-to-N",
        OutputMode::TopN => "top-n",
    }
}
