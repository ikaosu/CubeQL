//! Tauri IPC message types.
//!
//! `SearchRequest` is re-exported from `cube_core::search::request` so that the
//! Tauri command and `cube-cli bench` build SearchInputs through the same code
//! path (memory `feedback_bench_must_hit_real_ui_path`).
//!
//! All wire field names use camelCase to match `src/shared/messages.ts`.

use serde::Serialize;

pub use cube_core::search::SearchRequest;

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum WorkerEvent {
    Progress {
        #[serde(rename = "jobId")]
        job_id: String,
        depth: usize,
        #[serde(rename = "solutionsFound")]
        solutions_found: usize,
        #[serde(rename = "nodesExplored")]
        nodes_explored: usize,
        #[serde(rename = "elapsedMs")]
        elapsed_ms: u128,
    },
    Solution {
        #[serde(rename = "jobId")]
        job_id: String,
        moves: Vec<usize>,
        length: usize,
    },
    Complete {
        #[serde(rename = "jobId")]
        job_id: String,
        #[serde(rename = "totalSolutions")]
        total_solutions: usize,
        #[serde(rename = "timeMs")]
        time_ms: u128,
        #[serde(rename = "nodesExplored")]
        nodes_explored: usize,
        #[serde(rename = "shortestLength")]
        shortest_length: Option<usize>,
    },
    Error {
        #[serde(rename = "jobId")]
        job_id: String,
        message: String,
    },
    #[serde(rename = "table-build-start")]
    TableBuildStart {
        step: usize,
        total: usize,
        name: String,
        source: &'static str,
    },
    #[serde(rename = "table-build-step-done")]
    TableBuildStepDone {
        step: usize,
        total: usize,
        name: String,
        #[serde(rename = "elapsedMs")]
        elapsed_ms: u128,
    },
    #[serde(rename = "table-build-all-done")]
    TableBuildAllDone {
        #[serde(rename = "elapsedMs")]
        elapsed_ms: u128,
    },
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectOpenResult {
    pub path: String,
    pub data: String,
}
