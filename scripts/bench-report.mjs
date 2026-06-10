#!/usr/bin/env node
// bench-report.mjs
//
// Join multiple bench CSVs (one per solver) on case_id and emit a markdown
// comparison report. Reads CSVs produced by `cube-cli bench` and
// `bench-cube-explorer.ps1` (same schema).
//
// Usage:
//   node bench-report.mjs --csv cube-explorer=path1.csv --csv cube-core=path2.csv \
//                         --baseline cube-explorer --out 04_test/bench-report.md
//
// Picks --baseline as the reference solver; for every other solver, the
// ratio column = (other.median_ms / baseline.median_ms).

import * as fs from "node:fs";

function parseArgs(argv) {
  const args = { csv: [], baseline: null, out: null };
  let i = 0;
  while (i < argv.length) {
    const a = argv[i];
    if (a === "--csv") {
      args.csv.push(argv[++i]);
    } else if (a === "--baseline") {
      args.baseline = argv[++i];
    } else if (a === "--out") {
      args.out = argv[++i];
    } else {
      console.error(`unknown arg ${a}`);
      process.exit(2);
    }
    i++;
  }
  if (args.csv.length === 0 || !args.out) {
    console.error(
      "usage: bench-report.mjs --csv <label>=<path> [--csv <label>=<path> ...] --baseline <label> --out <md>"
    );
    process.exit(2);
  }
  return args;
}

function parseCsv(text) {
  const lines = text.split(/\r?\n/).filter((l) => l.trim().length > 0);
  const header = lines[0].split(",");
  const idx = (name) => header.indexOf(name);
  const rows = lines.slice(1).map((line) => {
    // Naive CSV split — values containing commas would need full parser.
    const cells = line.split(",");
    const row = {};
    header.forEach((h, k) => (row[h] = cells[k]));
    return row;
  });
  return { header, rows, idx };
}

function loadCsvSpecs(specs) {
  // each spec: "label=path"
  const solvers = [];
  for (const spec of specs) {
    const eq = spec.indexOf("=");
    if (eq < 0) {
      console.error(`expected label=path, got ${spec}`);
      process.exit(2);
    }
    const label = spec.slice(0, eq);
    const path = spec.slice(eq + 1);
    const text = fs.readFileSync(path, "utf8");
    const { rows } = parseCsv(text);
    const byCase = new Map();
    for (const r of rows) {
      byCase.set(r.case_id, r);
    }
    solvers.push({ label, path, byCase, rows });
  }
  return solvers;
}

function fmtRatio(other, base) {
  if (base === 0) return other === 0 ? "—" : "∞";
  const r = other / base;
  if (!isFinite(r)) return "∞";
  return r.toFixed(2) + "×";
}

function categorize(rows) {
  const cats = new Map();
  for (const r of rows) {
    const c = r.category;
    if (!cats.has(c)) cats.set(c, []);
    cats.get(c).push(r);
  }
  return cats;
}

function statSummary(values) {
  if (values.length === 0) return { median: 0, mean: 0, max: 0, count: 0 };
  const sorted = [...values].sort((a, b) => a - b);
  const median = sorted[Math.floor(sorted.length / 2)];
  const mean = values.reduce((a, b) => a + b, 0) / values.length;
  const max = sorted[sorted.length - 1];
  return { median, mean, max, count: values.length };
}

function main() {
  const args = parseArgs(process.argv.slice(2));
  const solvers = loadCsvSpecs(args.csv);
  const baseline = solvers.find((s) => s.label === args.baseline);
  if (!baseline && args.baseline) {
    console.error(`baseline ${args.baseline} not in --csv list`);
    process.exit(2);
  }

  // Unioned case_id set in baseline order, then any extras.
  const allCases = new Set();
  if (baseline) for (const r of baseline.rows) allCases.add(r.case_id);
  for (const s of solvers) for (const r of s.rows) allCases.add(r.case_id);

  const lines = [];
  lines.push("# Bench report");
  lines.push("");
  lines.push(`Generated: ${new Date().toISOString()}`);
  lines.push("");
  lines.push("Source CSVs:");
  for (const s of solvers) {
    lines.push(`- **${s.label}** — \`${s.path}\` (${s.rows.length} rows)`);
  }
  if (baseline) {
    lines.push("");
    lines.push(`Baseline: **${baseline.label}** (ratios shown as candidate / baseline median_ms).`);
  }
  lines.push("");

  // ---- Per-category summary ----
  lines.push("## Summary by category");
  lines.push("");

  // Build per-case combined rows
  const combined = [];
  for (const id of allCases) {
    const row = { case_id: id };
    let category = null;
    for (const s of solvers) {
      const r = s.byCase.get(id);
      row[`${s.label}_median_ms`] = r ? Number(r.median_ms) : null;
      row[`${s.label}_status`] = r ? r.status : null;
      row[`${s.label}_first_length`] = r ? Number(r.first_length) : null;
      if (r && !category) category = r.category;
    }
    row.category = category || "unknown";
    combined.push(row);
  }

  const byCat = categorize(combined);
  // Stable ordering
  const catOrder = ["scramble", "oll", "pll", "f2l", "dsl", "impossibility"];
  const orderedCats = [
    ...catOrder.filter((c) => byCat.has(c)),
    ...[...byCat.keys()].filter((c) => !catOrder.includes(c)),
  ];

  // Header for summary
  const solverLabels = solvers.map((s) => s.label);
  let headerCells = ["category", "cases"];
  for (const lbl of solverLabels) {
    headerCells.push(`${lbl} median (ms)`);
    headerCells.push(`${lbl} max (ms)`);
  }
  if (baseline) {
    for (const s of solvers) {
      if (s.label === baseline.label) continue;
      headerCells.push(`${s.label}/${baseline.label} median ratio`);
    }
  }
  lines.push("| " + headerCells.join(" | ") + " |");
  lines.push("|" + headerCells.map(() => "---").join("|") + "|");

  for (const cat of orderedCats) {
    const rows = byCat.get(cat);
    const cells = [cat, String(rows.length)];
    const stats = {};
    for (const s of solverLabels) {
      const vals = rows
        .map((r) => r[`${s}_median_ms`])
        .filter((v) => v !== null && Number.isFinite(v));
      stats[s] = statSummary(vals);
      cells.push(String(Math.round(stats[s].median)));
      cells.push(String(Math.round(stats[s].max)));
    }
    if (baseline) {
      for (const s of solverLabels) {
        if (s === baseline.label) continue;
        if (stats[baseline.label].median > 0 && stats[s].median > 0) {
          cells.push(fmtRatio(stats[s].median, stats[baseline.label].median));
        } else {
          cells.push("—");
        }
      }
    }
    lines.push("| " + cells.join(" | ") + " |");
  }

  // ---- Per-case detail ----
  lines.push("");
  lines.push("## Per-case detail");
  lines.push("");
  for (const cat of orderedCats) {
    lines.push(`### ${cat}`);
    lines.push("");
    const rows = byCat.get(cat);
    const head = ["case_id"];
    for (const lbl of solverLabels) {
      head.push(`${lbl} ms`);
      head.push(`${lbl} status`);
      head.push(`${lbl} len`);
    }
    if (baseline) {
      for (const s of solverLabels) {
        if (s === baseline.label) continue;
        head.push(`${s}/${baseline.label}`);
      }
    }
    lines.push("| " + head.join(" | ") + " |");
    lines.push("|" + head.map(() => "---").join("|") + "|");
    for (const r of rows) {
      const cells = [r.case_id];
      for (const s of solverLabels) {
        const v = r[`${s}_median_ms`];
        cells.push(v === null ? "—" : String(v));
        cells.push(r[`${s}_status`] || "—");
        const len = r[`${s}_first_length`];
        cells.push(len === null || Number.isNaN(len) ? "—" : String(len));
      }
      if (baseline) {
        const bv = r[`${baseline.label}_median_ms`];
        for (const s of solverLabels) {
          if (s === baseline.label) continue;
          const ov = r[`${s}_median_ms`];
          if (bv !== null && ov !== null) {
            cells.push(fmtRatio(ov, bv));
          } else {
            cells.push("—");
          }
        }
      }
      lines.push("| " + cells.join(" | ") + " |");
    }
    lines.push("");
  }

  // ---- NF1 gate evaluation ----
  if (baseline) {
    lines.push("## NF1 gate evaluation (Cube Explorer-supported categories)");
    lines.push("");
    lines.push(
      "Per spec NF1: in categories Cube Explorer supports (`scramble`, `oll`, `pll`, `f2l`), the candidate solver's median time must be ≤ 1.0× the baseline (with p95 ≤ 1.5× tolerated)."
    );
    lines.push("");
    const supported = ["scramble", "oll", "pll", "f2l"];
    for (const s of solvers) {
      if (s.label === baseline.label) continue;
      lines.push(`### ${s.label}`);
      lines.push("");
      let supportedTotal = 0;
      let supportedPass = 0;
      const fails = [];
      for (const cat of supported) {
        const rows = byCat.get(cat) || [];
        for (const r of rows) {
          const bv = r[`${baseline.label}_median_ms`];
          const ov = r[`${s.label}_median_ms`];
          if (bv === null || ov === null) continue;
          supportedTotal++;
          if (ov <= bv * 1.0) {
            supportedPass++;
          } else {
            fails.push({ id: r.case_id, ratio: ov / bv, base: bv, candidate: ov });
          }
        }
      }
      lines.push(`- Cases checked: ${supportedTotal}`);
      lines.push(`- Median ≤ baseline: ${supportedPass} (${supportedTotal === 0 ? 0 : Math.round((100 * supportedPass) / supportedTotal)}%)`);
      if (fails.length > 0) {
        const worst = fails.sort((a, b) => b.ratio - a.ratio).slice(0, 10);
        lines.push("- Worst 10 (slowest relative to baseline):");
        for (const f of worst) {
          lines.push(`  - \`${f.id}\` — candidate ${f.candidate} ms vs baseline ${f.base} ms (${f.ratio.toFixed(2)}×)`);
        }
      }
      lines.push("");
    }
  }

  fs.writeFileSync(args.out, lines.join("\n"), "utf8");
  console.log(`wrote ${args.out}`);
}

main();
