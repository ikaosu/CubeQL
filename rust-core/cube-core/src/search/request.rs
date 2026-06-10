//! Shared `SearchRequest → SearchInput` construction.
//!
//! Both Tauri's `start_search` IPC and `cube-cli bench` build a SearchInput
//! by the same logic so that benchmarks reflect the actual UI path
//! (memory `feedback_bench_must_hit_real_ui_path`).

use serde::{Deserialize, Serialize};

use crate::dsl::parser::parse_pattern;
use crate::facelet::{create_solved_facelet, Facelet, FACELET_COUNT};
use crate::mask::{bake_in_mask, Mask};
use crate::output_mode::OutputMode;
use crate::search::SearchInput;

/// Wire-level search request. Field shape mirrors `src/shared/messages.ts`
/// `SearchRequest` so renderer → Tauri IPC works unchanged.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchRequest {
    pub job_id: String,
    pub initial: Vec<i8>,
    pub mask: Vec<u8>,
    pub dsl: String,
    pub max_moves: usize,
    pub output_mode: OutputMode,
    pub top_n: Option<usize>,
}

/// Build a `SearchInput` from a `SearchRequest`. Returns `Err(String)` on
/// validation failure (bad facelet length, bad mask length, DSL parse fail).
///
/// Logic:
/// 1. Validate `initial.len() == 54` and `mask.len() == 54`
/// 2. Parse DSL into `Pattern`
/// 3. `bake_in_mask(initial, mask)` → `EffectiveFacelet`
/// 4. Decide `use_pruning_table` based on whether the U-face is at all visible
///    (matches worker.ts and the prior Tauri `start_search` logic — full
///    don't-care of U-face would over-prune)
pub fn build_search_input(req: &SearchRequest) -> Result<SearchInput, String> {
    if req.initial.len() != FACELET_COUNT {
        return Err(format!(
            "facelet length mismatch: got {}, want {}",
            req.initial.len(),
            FACELET_COUNT
        ));
    }
    if req.mask.len() != FACELET_COUNT {
        return Err(format!(
            "mask length mismatch: got {}, want {}",
            req.mask.len(),
            FACELET_COUNT
        ));
    }

    let mut facelet: Facelet = [0i8; FACELET_COUNT];
    facelet.copy_from_slice(&req.initial);
    let mut mask_arr: Mask = [0u8; FACELET_COUNT];
    mask_arr.copy_from_slice(&req.mask);

    let pattern = parse_pattern(&req.dsl).map_err(|e| format!("CubeQL: {}", e))?;

    let effective = bake_in_mask(&facelet, &mask_arr);
    let goal = create_solved_facelet();

    // U-face visibility heuristic for pruning-table use. F2L (U-face fully
    // masked) would over-prune if PDB ran, so disable in that case.
    let u_face_visible = (0..9).any(|i| mask_arr[i] != 0);

    Ok(SearchInput {
        initial: effective,
        goal,
        pattern,
        max_moves: req.max_moves,
        output_mode: req.output_mode,
        top_n: req.top_n,
        use_pruning_table: u_face_visible,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::facelet::create_solved_facelet;

    fn req_basic() -> SearchRequest {
        let solved = create_solved_facelet();
        SearchRequest {
            job_id: "test".into(),
            initial: solved.to_vec(),
            mask: vec![1u8; FACELET_COUNT],
            dsl: "<U,D,R,L,F,B>".into(),
            max_moves: 20,
            output_mode: OutputMode::ShortestOnly,
            top_n: None,
        }
    }

    #[test]
    fn build_from_solved_state() {
        let req = req_basic();
        let input = build_search_input(&req).expect("build ok");
        assert_eq!(input.max_moves, 20);
        assert_eq!(input.use_pruning_table, true);
    }

    #[test]
    fn rejects_bad_facelet_len() {
        let mut req = req_basic();
        req.initial = vec![0i8; 50];
        assert!(build_search_input(&req).is_err());
    }

    #[test]
    fn rejects_bad_mask_len() {
        let mut req = req_basic();
        req.mask = vec![1u8; 50];
        assert!(build_search_input(&req).is_err());
    }

    #[test]
    fn rejects_bad_dsl() {
        let mut req = req_basic();
        req.dsl = "this is not valid".into();
        assert!(build_search_input(&req).is_err());
    }

    #[test]
    fn pruning_table_disabled_when_u_face_fully_masked() {
        let mut req = req_basic();
        // Mask out U-face (positions 0..9)
        for i in 0..9 {
            req.mask[i] = 0;
        }
        let input = build_search_input(&req).expect("build ok");
        assert_eq!(input.use_pruning_table, false);
    }
}
