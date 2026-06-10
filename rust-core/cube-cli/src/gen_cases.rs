//! Deterministic generator for bench case JSON files.
//!
//! `cube-cli gen-cases --out-dir <path> [--seed N]`
//!
//! Produces:
//!   scrambles_100.json   100 WCA-style random scrambles (seeded LCG)
//!   oll_57.json          57 OLL cases (setup = inverse of the alg applied
//!                                       to solved → state needs that alg)
//!   pll_21.json          21 PLL cases
//!   f2l_20.json          20 representative F2L cases
//!   dsl_15.json          15 DSL / mask / impossibility cases
//!   bench_full.json      union of all of the above

use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

use serde_json::json;

use cube_core::moves::parse_move_token;

const SCRAMBLE_LEN: usize = 25;
const SCRAMBLE_COUNT: usize = 100;

// Face indices for scramble generation: U(0) D(1) R(2) L(3) F(4) B(5).
const SCRAMBLE_FACES: [&str; 6] = ["U", "D", "R", "L", "F", "B"];
const SCRAMBLE_VARIANTS: [&str; 3] = ["", "'", "2"];

// Same-axis pairs (skip consecutive same-axis moves).
fn same_axis(a: usize, b: usize) -> bool {
    // U/D same axis, R/L same axis, F/B same axis.
    a / 2 == b / 2
}

pub fn cmd_gen_cases(args: &[String]) -> ExitCode {
    let mut out_dir: Option<String> = None;
    let mut seed: u64 = 20260518;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--out-dir" => {
                i += 1;
                out_dir = Some(args[i].clone());
            }
            "--seed" => {
                i += 1;
                seed = args[i].parse().unwrap_or(20260518);
            }
            other => {
                eprintln!("gen-cases: unknown arg \"{}\"", other);
                return ExitCode::from(2);
            }
        }
        i += 1;
    }
    let out_dir = match out_dir {
        Some(d) => PathBuf::from(d),
        None => {
            eprintln!("gen-cases: --out-dir <path> required");
            return ExitCode::from(2);
        }
    };
    if let Err(e) = fs::create_dir_all(&out_dir) {
        eprintln!("gen-cases: create {}: {}", out_dir.display(), e);
        return ExitCode::from(2);
    }

    // Write each file.
    // OLL/PLL/F2L are emitted in **full-solve** form (preset=null, default
    // HTM DSL) so they exercise the Kociemba fast path — apples-to-apples
    // with Cube Explorer which also full-solves any state submitted via its
    // HTTP API.
    write_file(&out_dir, "scrambles_100.json", make_scrambles(seed));
    write_file(&out_dir, "oll_57.json", make_oll_cases_fullsolve());
    write_file(&out_dir, "pll_21.json", make_pll_cases_fullsolve());
    write_file(&out_dir, "f2l_20.json", make_f2l_cases_fullsolve());
    write_file(&out_dir, "dsl_15.json", make_dsl_cases());

    // Union file (used by `cube-cli bench` and `bench-cube-explorer.ps1`).
    let mut all = Vec::new();
    all.extend(make_scrambles(seed));
    all.extend(make_oll_cases_fullsolve());
    all.extend(make_pll_cases_fullsolve());
    all.extend(make_f2l_cases_fullsolve());
    all.extend(make_dsl_cases());
    write_file(&out_dir, "bench_full.json", all);

    // Receipts file: masked versions of representative cases (Sune for OLL,
    // U-perm for PLL, one F2L slot). Verifies spec acceptance criteria:
    // the engine should find the optimal short alg when given a mask.
    write_file(&out_dir, "receipts.json", make_receipts_cases());

    // UI-default-based case set: reflects what the user actually triggers
    // from the React UI (preset selected, AUF DSL, preset-specific
    // maxMoves). NF1 gate per plan v3 uses this file.
    write_file(&out_dir, "bench_ui.json", make_ui_default_cases(seed));

    ExitCode::from(0)
}

fn write_file(dir: &PathBuf, name: &str, cases: Vec<serde_json::Value>) {
    let payload = json!({ "version": 1, "cases": cases });
    let path = dir.join(name);
    let text = serde_json::to_string_pretty(&payload).unwrap();
    if let Err(e) = fs::write(&path, text) {
        eprintln!("gen-cases: write {}: {}", path.display(), e);
        return;
    }
    eprintln!("gen-cases: wrote {} ({} cases)", path.display(), cases.len());
}

// ---------- Scramble generation (LCG-seeded) ----------

struct Lcg {
    state: u64,
}
impl Lcg {
    fn new(seed: u64) -> Self {
        Self { state: seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407) }
    }
    fn next_u32(&mut self) -> u32 {
        self.state = self
            .state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.state >> 32) as u32
    }
    fn next_usize(&mut self, n: usize) -> usize {
        (self.next_u32() as usize) % n
    }
}

fn random_scramble(rng: &mut Lcg, len: usize) -> String {
    let mut out = String::new();
    let mut last_face: Option<usize> = None;
    let mut second_last_face: Option<usize> = None;
    while out.split_whitespace().count() < len {
        let f = rng.next_usize(6);
        if let Some(lf) = last_face {
            if f == lf {
                continue;
            }
            // Skip if both previous moves are on the same axis (e.g. R L R is fine,
            // but R R can't happen because of above check; here we further block
            // R L R as wasteful — optional but standard).
            if let Some(sf) = second_last_face {
                if same_axis(f, lf) && same_axis(lf, sf) {
                    continue;
                }
            }
        }
        let v = rng.next_usize(3);
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(SCRAMBLE_FACES[f]);
        out.push_str(SCRAMBLE_VARIANTS[v]);
        second_last_face = last_face;
        last_face = Some(f);
    }
    out
}

fn make_scrambles(seed: u64) -> Vec<serde_json::Value> {
    let mut rng = Lcg::new(seed);
    let mut cases = Vec::with_capacity(SCRAMBLE_COUNT);
    for i in 0..SCRAMBLE_COUNT {
        let scramble = random_scramble(&mut rng, SCRAMBLE_LEN);
        cases.push(json!({
            "id": format!("scr-{:03}", i + 1),
            "category": "scramble",
            "scramble": scramble,
            "preset": null,
            "dsl": "<U,D,R,L,F,B>",
            "max_moves": 22,
            "output_mode": "shortest-only",
            "top_n": null,
            "expect_solutions": null,
            "expect_first_length_max": 22
        }));
    }
    cases
}

// ---------- OLL / PLL / F2L data (well-known standard algorithms) ----------
//
// For OLL/PLL: setup scramble = the alg itself applied to a solved cube,
// since applying the alg to its own state takes you back to solved-with-alg.
// More precisely: to *generate* an OLL state, apply the inverse of the alg to
// a solved cube. But many cubers define the "case" by the alg that solves it,
// so we use the inverse-applied-to-solved as the setup.
//
// To keep determinism and avoid embedding 57 inverses by hand, we apply the
// forward alg to solved — this gives a state that is *also* a valid OLL case
// (just shifted). For benchmarking the engine's speed this is sufficient.

// ===== OLL 57 (standard set) =====
// Source: widely-published OLL list. Cases ordered by canonical number.
const OLL_ALGS: &[(u32, &str, &str)] = &[
    // (case_number, name_shorthand, algorithm)
    (1, "Dot",             "R U2 R2 F R F' U2 R' F R F'"),
    (2, "Dot",             "F R U R' U' F' f R U R' U' f'"),
    (3, "Dot",             "f R U R' U' f' U' F R U R' U' F'"),
    (4, "Dot",             "f R U R' U' f' U F R U R' U' F'"),
    (5, "SquareShape",     "r' U2 R U R' U r"),
    (6, "SquareShape",     "r U2 R' U' R U' r'"),
    (7, "SmallLightning",  "r U R' U R U2 r'"),
    (8, "SmallLightning",  "r' U' R U' R' U2 r"),
    (9, "FishShape",       "R U R' U' R' F R2 U R' U' F'"),
    (10, "FishShape",      "R U R' U R' F R F' R U2 R'"),
    (11, "SmallLightning", "r' R2 U R' U R U2 R' U M'"),
    (12, "SmallLightning", "M' R' U' R U' R' U2 R U' R r'"),
    (13, "Knight",         "F U R U' R2 F' R U R U' R'"),
    (14, "Knight",         "R' F R U R' F' R F U' F'"),
    (15, "Knight",         "r' U' r R' U' R U r' U r"),
    (16, "Knight",         "r U r' R U R' U' r U' r'"),
    (17, "Dot",             "R U R' U R' F R F' U2 R' F R F'"),
    (18, "Dot",             "r U R' U R U2 r2 U' R U' R' U2 r"),
    (19, "Dot",             "r' R U R U R' U' M' R' F R F'"),
    (20, "Dot",             "r U R' U' M2 U R U' R' U' M'"),
    (21, "OCLL",            "R U2 R' U' R U R' U' R U' R'"),
    (22, "OCLL",            "R U2 R2 U' R2 U' R2 U2 R"),
    (23, "OCLL",            "R2 D' R U2 R' D R U2 R"),
    (24, "OCLL",            "r U R' U' r' F R F'"),
    (25, "OCLL",            "F' r U R' U' r' F R"),
    (26, "OCLL",            "R U2 R' U' R U' R'"),
    (27, "Sune",            "R U R' U R U2 R'"),
    (28, "Knight",          "r U R' U' r' R U R U' R'"),
    (29, "Awkward",         "R U R' U' R U' R' F' U' F R U R'"),
    (30, "Awkward",         "F R' F R2 U' R' U' R U R' F2"),
    (31, "SmallLightning",  "R' U' F U R U' R' F' R"),
    (32, "SmallLightning",  "L U F' U' L' U L F L'"),
    (33, "T-shape",         "R U R' U' R' F R F'"),
    (34, "C-shape",         "R U R2 U' R' F R U R U' F'"),
    (35, "FishShape",       "R U2 R2 F R F' R U2 R'"),
    (36, "WShape",          "L' U' L U' L' U L U L F' L' F"),
    (37, "FishShape",       "F R' F' R U R U' R'"),
    (38, "WShape",          "R U R' U R U' R' U' R' F R F'"),
    (39, "BigLightning",    "L F' L' U' L U F U' L'"),
    (40, "BigLightning",    "R' F R U R' U' F' U R"),
    (41, "Awkward",         "R U R' U R U2 R' F R U R' U' F'"),
    (42, "Awkward",         "R' U' R U' R' U2 R F R U R' U' F'"),
    (43, "P-shape",         "f' L' U' L U f"),
    (44, "P-shape",         "f R U R' U' f'"),
    (45, "T-shape",         "F R U R' U' F'"),
    (46, "C-shape",         "R' U' R' F R F' U R"),
    (47, "BigLightning",    "R' U' R' F R F' R' F R F' U R"),
    (48, "BigLightning",    "F R U R' U' R U R' U' F'"),
    (49, "Awkward",         "r U' r2 U r2 U r2 U' r"),
    (50, "Awkward",         "r' U r2 U' r2 U' r2 U r'"),
    (51, "P-shape",         "f R U R' U' R U R' U' f'"),
    (52, "P-shape",         "R U R' U R U' B U' B' R'"),
    (53, "Awkward",         "r' U' R U' R' U R U' R' U2 r"),
    (54, "Awkward",         "r U R' U R U' R' U R U2 r'"),
    (55, "I-shape",         "R U2 R2 U' R U' R' U2 F R F'"),
    (56, "I-shape",         "r U r' U R U' R' U R U' R' r U' r'"),
    (57, "H-shape",         "R U R' U' M' U R U' r'"),
];

const PLL_ALGS: &[(&str, &str)] = &[
    ("Aa", "x R' U R' D2 R U' R' D2 R2 x'"),
    ("Ab", "x R2 D2 R U R' D2 R U' R x'"),
    ("E",  "x' R U' R' D R U R' D' R U R' D R U' R' D' x"),
    ("F",  "R' U' F' R U R' U' R' F R2 U' R' U' R U R' U R"),
    ("Ga", "R2 U R' U R' U' R U' R2 U' D R' U R D'"),
    ("Gb", "F' U' F R2 U R' U R U' R U' R2 D' U R U' D"),
    ("Gc", "R2 U' R U' R U R' U R2 U D' R U' R' D"),
    ("Gd", "R U R' U' D R2 U' R U' R' U R' U R2 D'"),
    ("H",  "M2 U M2 U2 M2 U M2"),
    ("Ja", "R' U L' U2 R U' R' U2 R L"),
    ("Jb", "R U R' F' R U R' U' R' F R2 U' R'"),
    ("Na", "R U R' U R U R' F' R U R' U' R' F R2 U' R'"),
    ("Nb", "R' U R U' R' F' U' F R U R' F R' F' R U' R"),
    ("Ra", "R U' R' U' R U R D R' U' R D' R' U2 R'"),
    ("Rb", "R' U2 R U2 R' F R U R' U' R' F' R2"),
    ("T",  "R U R' U' R' F R2 U' R' U' R U R' F'"),
    ("Ua", "M2 U M U2 M' U M2"),
    ("Ub", "M2 U' M U2 M' U' M2"),
    ("V",  "R' U R' U' y R' F' R2 U' R' U R' F R F"),
    ("Y",  "F R U' R' U' R U R' F' R U R' U' R' F R F'"),
    ("Z",  "M' U M2 U M2 U M' U2 M2"),
];

// 20 representative F2L cases (slot inserts / reorientations).
const F2L_ALGS: &[(u32, &str)] = &[
    (1,  "U R U' R'"),
    (2,  "U' F' U F"),
    (3,  "U' R U R' U2 R U' R'"),
    (4,  "U F' U' F U2 F' U F"),
    (5,  "R U' R' U R U' R'"),
    (6,  "F' U F U' F' U F"),
    (7,  "U R U2 R' U R U' R'"),
    (8,  "U' F' U2 F U' F' U F"),
    (9,  "y' U' R' U R U' R' U R"),
    (10, "U' R U' R' U R U' R'"),
    (11, "U R U R' U' R U R'"),
    (12, "U' F' U' F U F' U' F"),
    (13, "U R U2 R' U2 R U' R'"),
    (14, "U' R' U R U2 R' U R"),
    (15, "R U' R' U' R U R' U2 R U' R'"),
    (16, "R U R' U2 R U2 R'"),
    (17, "y' R' U2 R U R' U' R"),
    (18, "R U R' U' R U' R' U R U R'"),
    (19, "U' R U2 R' U R U R'"),
    (20, "U' R U R' U R U R'"),
];

// ===== Full-solve variants of OLL/PLL/F2L (for NF1 parity bench) =====
// Cube Explorer's HTTP API always full-solves the submitted facelet to a
// solved cube — there is no mask hookup. To compare apples-to-apples,
// the bench cases emit full-solve queries (preset=null, default HTM DSL,
// shortest-only, max_moves=22). cube-core dispatches these to min2phase.

fn make_oll_cases_fullsolve() -> Vec<serde_json::Value> {
    OLL_ALGS
        .iter()
        .filter_map(|(num, name, alg)| {
            let normalized = normalize_wide_aliases(alg);
            if !validate_scramble(&normalized) {
                eprintln!("WARN: OLL #{} has invalid moves, skipping", num);
                return None;
            }
            Some(json!({
                "id": format!("oll-{:02}-{}", num, name),
                "category": "oll",
                "scramble": normalized,
                "preset": null,
                "dsl": "<U,D,R,L,F,B>",
                "max_moves": 22,
                "output_mode": "shortest-only",
                "top_n": null,
                "expect_solutions": null,
                "expect_first_length_max": 22
            }))
        })
        .collect()
}

fn make_pll_cases_fullsolve() -> Vec<serde_json::Value> {
    PLL_ALGS
        .iter()
        .filter_map(|(name, alg)| {
            let normalized = normalize_wide_aliases(alg);
            if !validate_scramble(&normalized) {
                eprintln!("WARN: PLL {} has invalid moves, skipping", name);
                return None;
            }
            Some(json!({
                "id": format!("pll-{}", name),
                "category": "pll",
                "scramble": normalized,
                "preset": null,
                "dsl": "<U,D,R,L,F,B>",
                "max_moves": 22,
                "output_mode": "shortest-only",
                "top_n": null,
                "expect_solutions": null,
                "expect_first_length_max": 22
            }))
        })
        .collect()
}

fn make_f2l_cases_fullsolve() -> Vec<serde_json::Value> {
    F2L_ALGS
        .iter()
        .filter_map(|(num, alg)| {
            let normalized = normalize_wide_aliases(alg);
            if !validate_scramble(&normalized) {
                eprintln!("WARN: F2L #{} has invalid moves, skipping", num);
                return None;
            }
            Some(json!({
                "id": format!("f2l-{:02}", num),
                "category": "f2l",
                "scramble": normalized,
                "preset": null,
                "dsl": "<U,D,R,L,F,B>",
                "max_moves": 22,
                "output_mode": "shortest-only",
                "top_n": null,
                "expect_solutions": null,
                "expect_first_length_max": 22
            }))
        })
        .collect()
}

/// Spec acceptance receipt cases — masked versions of representative
/// algorithms. Verifies the engine can find the short optimal alg when
/// given a mask (uses the IDA*+PDB engine, not min2phase). These are NOT
/// part of the NF1 parity bench (CE has no equivalent).
fn make_receipts_cases() -> Vec<serde_json::Value> {
    vec![
        json!({
            "id": "rcpt-oll-sune",
            "category": "receipt",
            "scramble": "R U R' U R U2 R'",
            "preset": "OLL",
            "dsl": "<U> <U,D,R,L,F,B> <U>",
            "max_moves": 9,
            "output_mode": "shortest-only",
            "top_n": null,
            "expect_solutions": null,
            "expect_first_length_max": 9
        }),
        json!({
            "id": "rcpt-pll-uperm-RU",
            "category": "receipt",
            "scramble": "R U' R U R U R U' R' U' R2",
            "preset": "PLL",
            "dsl": "<U> <R,U> <U>",
            "max_moves": 13,
            "output_mode": "shortest-only",
            "top_n": null,
            "expect_solutions": null,
            "expect_first_length_max": 13
        }),
        json!({
            "id": "rcpt-impossible-RU",
            "category": "receipt",
            "scramble": "F",
            "preset": null,
            "dsl": "<R,U>",
            "max_moves": 20,
            "output_mode": "enumerate-up-to-N",
            "top_n": null,
            "expect_solutions": 0,
            "expect_first_length_max": null
        }),
        json!({
            "id": "rcpt-quantifier-1-4",
            "category": "receipt",
            "scramble": "R U R'",
            "preset": null,
            "dsl": "<R,U>{1,4}",
            "max_moves": 4,
            "output_mode": "shortest-only",
            "top_n": null,
            "expect_solutions": null,
            "expect_first_length_max": 4
        }),
    ]
}

// ---------- DSL / impossibility / restriction cases ----------

fn make_dsl_cases() -> Vec<serde_json::Value> {
    vec![
        // dsl-01: <R,U> solves Sune
        json!({
            "id": "dsl-01-sune-RU",
            "category": "dsl",
            "scramble": "R U R' U R U2 R'",
            "preset": null,
            "dsl": "<R,U>",
            "max_moves": 8,
            "output_mode": "shortest-only",
            "top_n": null,
            "expect_solutions": null,
            "expect_first_length_max": 8
        }),
        // dsl-02: <R,U> solves U-perm (it does — Ua/Ub can be done with R,U,M but Ua is just RU)
        json!({
            "id": "dsl-02-uperm-RU",
            "category": "dsl",
            "scramble": "R U' R U R U R U' R' U' R2",
            "preset": null,
            "dsl": "<R,U>",
            "max_moves": 13,
            "output_mode": "shortest-only",
            "top_n": null,
            "expect_first_length_max": 13
        }),
        // dsl-03..07: impossibility — scrambles outside <R,U>
        json!({
            "id": "dsl-03-impossible-F",
            "category": "impossibility",
            "scramble": "F",
            "preset": null,
            "dsl": "<R,U>",
            "max_moves": 20,
            "output_mode": "enumerate-up-to-N",
            "top_n": null,
            "expect_solutions": 0
        }),
        json!({
            "id": "dsl-04-impossible-L",
            "category": "impossibility",
            "scramble": "L",
            "preset": null,
            "dsl": "<R,U>",
            "max_moves": 20,
            "output_mode": "enumerate-up-to-N",
            "top_n": null,
            "expect_solutions": 0
        }),
        json!({
            "id": "dsl-05-impossible-RUF",
            "category": "impossibility",
            "scramble": "R U F R' U'",
            "preset": null,
            "dsl": "<R,U>",
            "max_moves": 18,
            "output_mode": "enumerate-up-to-N",
            "top_n": null,
            "expect_solutions": 0
        }),
        json!({
            "id": "dsl-06-impossible-D",
            "category": "impossibility",
            "scramble": "D",
            "preset": null,
            "dsl": "<R,U>",
            "max_moves": 20,
            "output_mode": "enumerate-up-to-N",
            "top_n": null,
            "expect_solutions": 0
        }),
        json!({
            "id": "dsl-07-impossible-B2",
            "category": "impossibility",
            "scramble": "B2",
            "preset": null,
            "dsl": "<R,U>",
            "max_moves": 20,
            "output_mode": "enumerate-up-to-N",
            "top_n": null,
            "expect_solutions": 0
        }),
        // dsl-08..10: BaseLimits enforcing
        json!({
            "id": "dsl-08-baselimit-F-cap",
            "category": "dsl",
            "scramble": "R U R' U R U2 R'",
            "preset": null,
            "dsl": "[F*<=3] <R,U,F>{1,12}",
            "max_moves": 12,
            "output_mode": "shortest-only",
            "top_n": null,
            "expect_first_length_max": 12
        }),
        json!({
            "id": "dsl-09-baselimit-F-forbid",
            "category": "dsl",
            "scramble": "R U R' U R U2 R'",
            "preset": null,
            "dsl": "[F*<=0] <R,U,F,D>{1,12}",
            "max_moves": 12,
            "output_mode": "shortest-only",
            "top_n": null,
            "expect_first_length_max": 12
        }),
        json!({
            "id": "dsl-10-baselimit-D2-forbid",
            "category": "dsl",
            "scramble": "R U R' U' R' F R F'",
            "preset": null,
            "dsl": "[D2<=0] <R,U,F,D>{1,12}",
            "max_moves": 12,
            "output_mode": "shortest-only",
            "top_n": null,
            "expect_first_length_max": 12
        }),
        // dsl-11..13: custom mask (use OCLL preset as representative custom mask)
        json!({
            "id": "dsl-11-ocll-mask",
            "category": "dsl",
            "scramble": "R U2 R' U' R U' R'",
            "preset": "OCLL",
            "dsl": "<U> <U,D,R,L,F,B> <U>",
            "max_moves": 12,
            "output_mode": "shortest-only",
            "top_n": null,
            "expect_first_length_max": 12
        }),
        json!({
            "id": "dsl-12-eoll-mask",
            "category": "dsl",
            "scramble": "F R U R' U' F'",
            "preset": "EOLL",
            "dsl": "<U> <U,D,R,L,F,B> <U>",
            "max_moves": 12,
            "output_mode": "shortest-only",
            "top_n": null,
            "expect_first_length_max": 12
        }),
        json!({
            "id": "dsl-13-coll-mask",
            "category": "dsl",
            "scramble": "R U R' U R U2 R'",
            "preset": "COLL",
            "dsl": "<U> <U,D,R,L,F,B> <U>",
            "max_moves": 14,
            "output_mode": "shortest-only",
            "top_n": null,
            "expect_first_length_max": 14
        }),
        // dsl-14..15: quantifier bounds
        json!({
            "id": "dsl-14-quantifier-1-4",
            "category": "dsl",
            "scramble": "R U R'",
            "preset": null,
            "dsl": "<R,U>{1,4}",
            "max_moves": 4,
            "output_mode": "enumerate-up-to-N",
            "top_n": null,
            "expect_first_length_max": 4
        }),
        json!({
            "id": "dsl-15-quantifier-exact-3",
            "category": "dsl",
            "scramble": "R U R'",
            "preset": null,
            "dsl": "<R,U>{3}",
            "max_moves": 3,
            "output_mode": "enumerate-up-to-N",
            "top_n": null,
            "expect_first_length_max": 3
        }),
    ]
}

// ===== UI-default-based cases (plan v3 § B3) =====
// These represent what the user actually triggers from the React UI:
// - preset selected (mask applied)
// - AUF DSL wrappers <U> ... <U>
// - preset-appropriate maxMoves
//
// NF1 gate evaluation uses this file. cube-cli bench reads bench_ui.json,
// runs the same `build_search_input` path as Tauri's start_search, and the
// resulting CSV is compared against Cube Explorer baselines (regular HTTP
// for scramble, manual Optimal Solver + filter mode CSVs — see
// 04_test/bench-protocol.md).

fn make_ui_default_cases(seed: u64) -> Vec<serde_json::Value> {
    let mut out = Vec::new();

    // (1) Scramble at the OLD UI default maxMoves=11 — should return 0
    //     immediately via PDB LB or IDA*+PDB iterative deepening short-circuit
    //     (no full-HTM solution at depth ≤ 11 for a 25-move random scramble).
    {
        let mut rng = Lcg::new(seed);
        for i in 0..20 {
            let scramble = random_scramble(&mut rng, SCRAMBLE_LEN);
            out.push(json!({
                "id": format!("ui-scr-11-{:03}", i + 1),
                "category": "ui-scramble-11",
                "scramble": scramble,
                "preset": null,
                "dsl": "<U,D,R,L,F,B>",
                "max_moves": 11,
                "output_mode": "shortest-only",
                "top_n": null,
                "expect_solutions": 0,
                "expect_first_length_max": null
            }));
        }
    }

    // (2) Scramble at maxMoves=22 — main NF1 parity (vs CE Optimal Solver).
    //     Random scramble optimal ~17-22, IDA*+PDB iterative deepening with
    //     min2phase upper bound finishes in 数秒〜数十秒.
    {
        let mut rng = Lcg::new(seed.wrapping_add(1));
        for i in 0..20 {
            let scramble = random_scramble(&mut rng, SCRAMBLE_LEN);
            out.push(json!({
                "id": format!("ui-scr-22-{:03}", i + 1),
                "category": "ui-scramble-22",
                "scramble": scramble,
                "preset": null,
                "dsl": "<U,D,R,L,F,B>",
                "max_moves": 22,
                "output_mode": "shortest-only",
                "top_n": null,
                "expect_solutions": null,
                "expect_first_length_max": 22
            }));
        }
    }

    // (3) OLL preset + AUF DSL — CE filter mode equivalent
    for (num, name, alg) in OLL_ALGS.iter() {
        let normalized = normalize_wide_aliases(alg);
        if !validate_scramble(&normalized) {
            continue;
        }
        out.push(json!({
            "id": format!("ui-oll-{:02}-{}", num, name),
            "category": "ui-oll-mask",
            "scramble": normalized,
            "preset": "OLL",
            "dsl": "<U> <U,D,R,L,F,B> <U>",
            "max_moves": 14,
            "output_mode": "shortest-only",
            "top_n": null,
            "expect_solutions": null,
            "expect_first_length_max": 14
        }));
    }

    // (4) PLL preset + AUF DSL
    for (name, alg) in PLL_ALGS.iter() {
        let normalized = normalize_wide_aliases(alg);
        if !validate_scramble(&normalized) {
            continue;
        }
        out.push(json!({
            "id": format!("ui-pll-{}", name),
            "category": "ui-pll-mask",
            "scramble": normalized,
            "preset": "PLL",
            "dsl": "<U> <U,D,R,L,F,B> <U>",
            "max_moves": 18,
            "output_mode": "shortest-only",
            "top_n": null,
            "expect_solutions": null,
            "expect_first_length_max": 18
        }));
    }

    // (5) F2L preset
    for (num, alg) in F2L_ALGS.iter() {
        let normalized = normalize_wide_aliases(alg);
        if !validate_scramble(&normalized) {
            continue;
        }
        out.push(json!({
            "id": format!("ui-f2l-{:02}", num),
            "category": "ui-f2l-mask",
            "scramble": normalized,
            "preset": "F2L",
            "dsl": "<U,D,R,L,F,B>",
            "max_moves": 18,
            "output_mode": "shortest-only",
            "top_n": null,
            "expect_solutions": null,
            "expect_first_length_max": 18
        }));
    }

    // (6) Impossibility (Level 1: basis-union outside subgroup)
    out.push(json!({
        "id": "ui-impossible-F-in-RU",
        "category": "ui-impossibility",
        "scramble": "F",
        "preset": null,
        "dsl": "<R,U>",
        "max_moves": 20,
        "output_mode": "enumerate-up-to-N",
        "top_n": null,
        "expect_solutions": 0,
        "expect_first_length_max": null
    }));
    out.push(json!({
        "id": "ui-impossible-D-in-RU",
        "category": "ui-impossibility",
        "scramble": "D",
        "preset": null,
        "dsl": "<R,U>",
        "max_moves": 20,
        "output_mode": "enumerate-up-to-N",
        "top_n": null,
        "expect_solutions": 0,
        "expect_first_length_max": null
    }));

    out
}

/// Convert lowercase wide-move aliases (`r u f d l b`) to the canonical
/// `Rw Uw Fw Dw Lw Bw` form that cube-core's parser accepts. M/E/S and
/// rotations (x/y/z) pass through unchanged.
fn normalize_wide_aliases(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for tok in s.replace('\u{2019}', "'").split_whitespace() {
        if !out.is_empty() {
            out.push(' ');
        }
        let mut chars = tok.chars();
        let first = chars.next().unwrap_or(' ');
        let rest: String = chars.collect();
        match first {
            'r' => { out.push_str("Rw"); out.push_str(&rest); }
            'l' => { out.push_str("Lw"); out.push_str(&rest); }
            'u' => { out.push_str("Uw"); out.push_str(&rest); }
            'd' => { out.push_str("Dw"); out.push_str(&rest); }
            'f' => { out.push_str("Fw"); out.push_str(&rest); }
            'b' => { out.push_str("Bw"); out.push_str(&rest); }
            _ => out.push_str(tok),
        }
    }
    out
}

fn validate_scramble(s: &str) -> bool {
    let normalized = normalize_wide_aliases(s);
    for tok in normalized.split_whitespace() {
        if parse_move_token(tok).is_none() {
            eprintln!("    invalid token: {}", tok);
            return false;
        }
    }
    true
}
