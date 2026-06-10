//! Headless CLI driver for cube-core.
//!
//! Usage:
//!   cube-cli solve <project.gencube.json>
//!   cube-cli build-tables
//!   cube-cli bench --cases <case-file.json> --out <csv> [--repeat N] [--warmup N] [--filter pat]
//!   cube-cli apply-scramble <scramble>           # print facelet string after scramble
//!
//! Prints solutions / progress to stdout. Mirrors the worker.ts contract but
//! talks to the local terminal instead of an IPC channel.

mod bench;
mod cubie_bench;
mod gen_cases;

use std::env;
use std::fs;
use std::process::ExitCode;

use cube_core::dsl::format::format_move_sequence;
use cube_core::dsl::parser::parse_pattern;
use cube_core::facelet::create_solved_facelet;
use cube_core::mask::bake_in_mask;
use cube_core::moves::{apply_moves, parse_move_token};
use cube_core::project::{facelet_to_string, parse_project, project_to_facelet, project_to_mask};
use cube_core::pruning::{get_pruning_tables_with_progress, BuildProgressEvent, BuildSource};
use cube_core::search::{run_search, SearchCallbacks, SearchInput};

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("usage: cube-cli <solve <path> | build-tables | bench ... | apply-scramble <scr>>");
        return ExitCode::from(2);
    }
    match args[1].as_str() {
        "solve" => {
            if args.len() < 3 {
                eprintln!("solve requires a project path");
                return ExitCode::from(2);
            }
            cmd_solve(&args[2])
        }
        "build-tables" => cmd_build_tables(),
        "bench" => bench::cmd_bench(&args[2..]),
        "cubie-bench" => cubie_bench::cmd_cubie_bench(&args[2..]),
        "gen-cases" => gen_cases::cmd_gen_cases(&args[2..]),
        "apply-scramble" => {
            let scramble = args[2..].join(" ");
            cmd_apply_scramble(&scramble)
        }
        other => {
            eprintln!("unknown subcommand: {}", other);
            ExitCode::from(2)
        }
    }
}

fn cmd_apply_scramble(scramble: &str) -> ExitCode {
    let normalized = scramble.replace('\u{2019}', "'");
    let mut moves: Vec<usize> = Vec::new();
    for tok in normalized.split_whitespace() {
        if tok.is_empty() {
            continue;
        }
        match parse_move_token(tok) {
            Some(m) => moves.push(m),
            None => {
                eprintln!("apply-scramble: unknown token \"{}\"", tok);
                return ExitCode::from(2);
            }
        }
    }
    let solved = create_solved_facelet();
    let scrambled = apply_moves(&solved, &moves);
    println!("{}", facelet_to_string(&scrambled));
    ExitCode::from(0)
}

fn report_progress(e: BuildProgressEvent) {
    match e {
        BuildProgressEvent::StepStart {
            step,
            total,
            name,
            source,
        } => {
            let src = match source {
                BuildSource::Cache => "cache",
                BuildSource::Build => "build",
            };
            println!("[{}/{}] {} ({})", step, total, name, src);
        }
        BuildProgressEvent::StepDone { name, elapsed_ms, .. } => {
            println!("    done in {} ms ({})", elapsed_ms, name);
        }
        BuildProgressEvent::AllDone { elapsed_ms } => {
            println!("pruning tables ready in {} ms", elapsed_ms);
        }
    }
}

fn cmd_build_tables() -> ExitCode {
    get_pruning_tables_with_progress(report_progress);
    ExitCode::from(0)
}

fn cmd_solve(path: &str) -> ExitCode {
    let text = match fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("read {}: {}", path, e);
            return ExitCode::from(2);
        }
    };
    let project = match parse_project(&text) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("parse {}: {}", path, e);
            return ExitCode::from(2);
        }
    };
    let facelet = project_to_facelet(&project);
    let mask = project_to_mask(&project);
    let initial = bake_in_mask(&facelet, &mask);
    let goal = create_solved_facelet();
    let pattern = match parse_pattern(&project.dsl) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("parse CubeQL: {}", e);
            return ExitCode::from(2);
        }
    };

    get_pruning_tables_with_progress(report_progress);
    let input = SearchInput {
        initial,
        goal,
        pattern,
        max_moves: project.search.max_moves,
        output_mode: project.search.output_mode,
        top_n: project.search.top_n,
        use_pruning_table: true,
    };
    let mut printed = 0usize;
    let cb = SearchCallbacks {
        on_solution: Some(Box::new(move |sol| {
            printed += 1;
            println!(
                "  solution #{:>3} (len {}): {}",
                printed,
                sol.length,
                format_move_sequence(&sol.moves)
            );
        })),
        on_progress: Some(Box::new(|p| {
            eprintln!(
                "  depth={} solutions={} nodes={} elapsed={}ms",
                p.depth, p.solutions_found, p.nodes_explored, p.elapsed_ms
            );
        })),
        should_cancel: None,
    };
    let stats = run_search(&input, cb);
    println!(
        "done — {} solutions, shortest={:?}, {} nodes, {} ms",
        stats.total_solutions, stats.shortest_length, stats.nodes_explored, stats.time_ms
    );
    ExitCode::from(0)
}
