//! `cube-cli bench` — run cube-core search over a fixed case set, output CSV.
//!
//! The bench harness is the entry point for NF1 (Cube Explorer parity)
//! verification. A second driver (PowerShell) queries Cube Explorer over its
//! HTTP API for the same case set; `bench-report.mjs` joins both CSVs.

use std::fs;
use std::path::Path;
use std::process::ExitCode;
use std::time::Instant;

use serde::{Deserialize, Serialize};

use cube_core::cubie_search::try_cubie_dispatch;
use cube_core::facelet::create_solved_facelet;
use cube_core::mask::{create_full_mask, Mask};
use cube_core::moves::{apply_moves, parse_move_token};
use cube_core::output_mode::OutputMode;
use cube_core::presets::mask_from_preset_id;
use cube_core::pruning::get_pruning_tables_with_progress;
use cube_core::search::{
    build_search_input, run_search, SearchCallbacks, SearchInput, SearchRequest,
};

// ---------- Case file schema ----------

#[derive(Debug, Clone, Deserialize)]
pub struct CaseFile {
    pub version: u32,
    pub cases: Vec<Case>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Case {
    pub id: String,
    pub category: String,
    pub scramble: String,
    pub preset: Option<String>,
    pub dsl: String,
    pub max_moves: usize,
    pub output_mode: OutputMode,
    pub top_n: Option<usize>,
    /// Optional assertion: expected solution count (e.g. 0 for impossibility).
    /// `None` skips the check.
    pub expect_solutions: Option<usize>,
    /// Optional assertion: cap on the length of the first emitted solution.
    /// `None` skips the check.
    pub expect_first_length_max: Option<usize>,
}

// ---------- CSV output ----------

#[derive(Debug, Clone, Serialize)]
struct Row {
    case_id: String,
    category: String,
    solver: String,
    runs: usize,
    median_ms: u128,
    p95_ms: u128,
    min_ms: u128,
    max_ms: u128,
    nodes: usize,
    solutions: usize,
    first_length: i64, // -1 if no solution
    status: String,    // "ok", "expected_zero_solutions", "assertion_failed:<...>", "error:<...>"
}

// ---------- CLI entry ----------

pub fn cmd_bench(args: &[String]) -> ExitCode {
    let mut cases_path: Option<String> = None;
    let mut out_path: Option<String> = None;
    let mut repeat: usize = 5;
    let mut warmup: usize = 1;
    let mut filter: Option<String> = None;
    let mut timeout_ms: u128 = 60_000;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--cases" => {
                i += 1;
                cases_path = Some(args[i].clone());
            }
            "--out" => {
                i += 1;
                out_path = Some(args[i].clone());
            }
            "--repeat" => {
                i += 1;
                repeat = args[i].parse().unwrap_or(5);
            }
            "--warmup" => {
                i += 1;
                warmup = args[i].parse().unwrap_or(1);
            }
            "--filter" => {
                i += 1;
                filter = Some(args[i].clone());
            }
            "--timeout-ms" => {
                i += 1;
                timeout_ms = args[i].parse().unwrap_or(60_000);
            }
            other => {
                eprintln!("bench: unknown arg \"{}\"", other);
                return ExitCode::from(2);
            }
        }
        i += 1;
    }
    let cases_path = match cases_path {
        Some(p) => p,
        None => {
            eprintln!("bench: --cases <path> required");
            return ExitCode::from(2);
        }
    };
    let out_path = match out_path {
        Some(p) => p,
        None => {
            eprintln!("bench: --out <csv> required");
            return ExitCode::from(2);
        }
    };

    let cases_text = match fs::read_to_string(&cases_path) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("bench: read {}: {}", cases_path, e);
            return ExitCode::from(2);
        }
    };
    let file: CaseFile = match serde_json::from_str(&cases_text) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("bench: parse {}: {}", cases_path, e);
            return ExitCode::from(2);
        }
    };
    if file.version != 1 {
        eprintln!("bench: unsupported case file version {}", file.version);
        return ExitCode::from(2);
    }

    eprintln!("bench: warming up pruning tables...");
    let t0 = Instant::now();
    get_pruning_tables_with_progress(|_| {});
    eprintln!("bench: tables ready in {} ms", t0.elapsed().as_millis());

    let mut rows: Vec<Row> = Vec::new();
    let total = file.cases.len();
    for (idx, case) in file.cases.iter().enumerate() {
        if let Some(f) = filter.as_ref() {
            if !case.id.contains(f.as_str()) && !case.category.contains(f.as_str()) {
                continue;
            }
        }
        let row = run_case(case, repeat, warmup, timeout_ms);
        eprintln!(
            "[{}/{}] {} ({})  median {} ms  solutions {}  status {}",
            idx + 1,
            total,
            case.id,
            case.category,
            row.median_ms,
            row.solutions,
            row.status
        );
        rows.push(row);
    }

    if let Err(e) = write_csv(&out_path, &rows) {
        eprintln!("bench: write {}: {}", out_path, e);
        return ExitCode::from(2);
    }
    eprintln!("bench: wrote {} ({} rows)", out_path, rows.len());
    ExitCode::from(0)
}

// ---------- per-case runner ----------

fn run_case(case: &Case, repeat: usize, warmup: usize, timeout_ms: u128) -> Row {
    let solver = "cube-core".to_string();
    // Build initial state by applying scramble to solved.
    let scramble_moves = match parse_scramble(&case.scramble) {
        Ok(m) => m,
        Err(e) => {
            return Row {
                case_id: case.id.clone(),
                category: case.category.clone(),
                solver,
                runs: 0,
                median_ms: 0,
                p95_ms: 0,
                min_ms: 0,
                max_ms: 0,
                nodes: 0,
                solutions: 0,
                first_length: -1,
                status: format!("error:parse_scramble:{}", e),
            };
        }
    };
    let solved = create_solved_facelet();
    let scrambled = apply_moves(&solved, &scramble_moves);

    let mask: Mask = match case.preset.as_deref() {
        Some(id) => match mask_from_preset_id(id) {
            Some(m) => m,
            None => {
                return error_row(
                    case,
                    solver,
                    format!("error:unknown_preset:{}", id),
                );
            }
        },
        None => create_full_mask(),
    };

    // Use the shared `build_search_input` so this exercises the **same code
    // path** as Tauri's `start_search` IPC handler (per
    // `feedback_bench_must_hit_real_ui_path` after INC-2026-05-18-01).
    let request = SearchRequest {
        job_id: case.id.clone(),
        initial: scrambled.to_vec(),
        mask: mask.to_vec(),
        dsl: case.dsl.clone(),
        max_moves: case.max_moves,
        output_mode: case.output_mode,
        top_n: case.top_n,
    };
    let input = match build_search_input(&request) {
        Ok(input) => input,
        Err(e) => {
            return error_row(case, solver, format!("error:build_search_input:{}", e));
        }
    };

    // Warmup runs (timing discarded). These also surface panics early.
    for _ in 0..warmup {
        let _ = run_once(&input, &request, timeout_ms);
    }
    let mut samples: Vec<RunOutcome> = Vec::with_capacity(repeat);
    let mut timed_out = false;
    for _ in 0..repeat {
        let r = run_once(&input, &request, timeout_ms);
        if r.timed_out {
            timed_out = true;
            samples.push(r);
            break; // No point repeating a timeout
        }
        samples.push(r);
    }
    samples.sort_by_key(|r| r.time_ms);

    let median = samples[samples.len() / 2].time_ms;
    let p95_idx = ((samples.len() as f64 - 1.0) * 0.95).round() as usize;
    let p95 = samples[p95_idx.min(samples.len() - 1)].time_ms;
    let min = samples.first().unwrap().time_ms;
    let max = samples.last().unwrap().time_ms;
    let nodes = samples[samples.len() / 2].nodes;
    let solutions = samples[samples.len() / 2].solutions;
    let first_length: i64 = samples[samples.len() / 2]
        .first_length
        .map(|v| v as i64)
        .unwrap_or(-1);

    // Assertions
    let mut status = if timed_out { "timeout".to_string() } else { "ok".to_string() };
    if let Some(expected) = case.expect_solutions {
        if solutions != expected {
            status = format!("assertion_failed:expected_solutions={} got={}", expected, solutions);
        } else if expected == 0 {
            status = "expected_zero_solutions".to_string();
        }
    }
    if let Some(max_len) = case.expect_first_length_max {
        if let Some(len) = samples[samples.len() / 2].first_length {
            if len > max_len {
                status = format!(
                    "assertion_failed:first_length={}>max={}",
                    len, max_len
                );
            }
        } else if status == "ok" {
            // No solution found but a length was expected.
            status = format!("assertion_failed:no_solution_but_expected_<={}", max_len);
        }
    }

    Row {
        case_id: case.id.clone(),
        category: case.category.clone(),
        solver,
        runs: samples.len(),
        median_ms: median,
        p95_ms: p95,
        min_ms: min,
        max_ms: max,
        nodes,
        solutions,
        first_length,
        status,
    }
}

struct RunOutcome {
    time_ms: u128,
    nodes: usize,
    solutions: usize,
    first_length: Option<usize>,
    timed_out: bool,
}

fn run_once(input: &SearchInput, request: &SearchRequest, timeout_ms: u128) -> RunOutcome {
    use std::sync::{Arc, Mutex};
    let first_len: Arc<Mutex<Option<usize>>> = Arc::new(Mutex::new(None));
    let deadline = Instant::now() + std::time::Duration::from_millis(timeout_ms as u64);

    // Try cubie engine first (Phase v4-9 dispatch). Falls through to legacy
    // if the request shape doesn't fit.
    let t0 = Instant::now();
    {
        let first_clone = Arc::clone(&first_len);
        let mut emit = move |length: usize, _moves: &[usize]| {
            let mut g = first_clone.lock().unwrap();
            if g.is_none() {
                *g = Some(length);
            }
        };
        let cancel_arc = Arc::new(move || Instant::now() >= deadline);
        let cancel_for_cubie = Arc::clone(&cancel_arc);
        let cancel_fn: &(dyn Fn() -> bool + Sync) = cancel_for_cubie.as_ref();
        if let Some(stats) = cube_core::cubie_search::try_cubie_dispatch(
            request,
            Some(&mut emit),
            cancel_fn,
        ) {
            let elapsed = t0.elapsed().as_millis();
            let first = *first_len.lock().unwrap();
            let timed_out = elapsed >= timeout_ms;
            return RunOutcome {
                time_ms: elapsed,
                nodes: stats.nodes_explored,
                solutions: stats.total_solutions,
                first_length: first,
                timed_out,
            };
        }
    }

    // Legacy facelet engine fallback
    let first_clone = Arc::clone(&first_len);
    let cb = SearchCallbacks {
        on_solution: Some(Box::new(move |sol| {
            let mut g = first_clone.lock().unwrap();
            if g.is_none() {
                *g = Some(sol.length);
            }
        })),
        on_progress: None,
        should_cancel: Some(Box::new(move || Instant::now() >= deadline)),
    };
    let stats = run_search(input, cb);
    let elapsed = t0.elapsed().as_millis();
    let first = *first_len.lock().unwrap();
    let timed_out = elapsed >= timeout_ms;
    RunOutcome {
        time_ms: elapsed,
        nodes: stats.nodes_explored,
        solutions: stats.total_solutions,
        first_length: first,
        timed_out,
    }
}

fn error_row(case: &Case, solver: String, status: String) -> Row {
    Row {
        case_id: case.id.clone(),
        category: case.category.clone(),
        solver,
        runs: 0,
        median_ms: 0,
        p95_ms: 0,
        min_ms: 0,
        max_ms: 0,
        nodes: 0,
        solutions: 0,
        first_length: -1,
        status,
    }
}

// ---------- helpers ----------

fn parse_scramble(s: &str) -> Result<Vec<usize>, String> {
    let mut out: Vec<usize> = Vec::new();
    let normalized = s.replace('\u{2019}', "'"); // Unicode prime → ASCII
    for tok in normalized.split_whitespace() {
        if tok.is_empty() {
            continue;
        }
        match parse_move_token(tok) {
            Some(m) => out.push(m),
            None => return Err(format!("unknown move token \"{}\"", tok)),
        }
    }
    Ok(out)
}

fn write_csv(path: &str, rows: &[Row]) -> Result<(), String> {
    if let Some(parent) = Path::new(path).parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let mut out = String::new();
    out.push_str("case_id,category,solver,runs,median_ms,p95_ms,min_ms,max_ms,nodes,solutions,first_length,status\n");
    for r in rows {
        out.push_str(&format!(
            "{},{},{},{},{},{},{},{},{},{},{},{}\n",
            csv_escape(&r.case_id),
            csv_escape(&r.category),
            csv_escape(&r.solver),
            r.runs,
            r.median_ms,
            r.p95_ms,
            r.min_ms,
            r.max_ms,
            r.nodes,
            r.solutions,
            r.first_length,
            csv_escape(&r.status),
        ));
    }
    fs::write(path, out).map_err(|e| e.to_string())
}

fn csv_escape(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') {
        let escaped = s.replace('"', "\"\"");
        format!("\"{}\"", escaped)
    } else {
        s.to_string()
    }
}
