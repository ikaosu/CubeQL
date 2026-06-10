import React, { useEffect, useMemo, useRef, useState } from "react";
import { FaceletEditor } from "./components/FaceletEditor";
import { CubeQLHelp } from "./components/CubeQLHelp";
import {
  Color,
  COLOR_NAMES,
  type Color as ColorT,
  type Mask,
  type OutputMode,
  applyMoves,
  applyMovesToMask,
  buildProjectJson,
  createFullMask,
  createSolvedFacelet,
  formatMoveSequence,
  getPresets,
  maskFromPresetId,
  parseMoveToken,
  parsePatternPreview,
  parseProject,
  presetById,
  validateFacelet,
} from "./cube-engine";
import type { FromWorker } from "../shared/messages";

type EditorMode = "color" | "mask";

interface SolutionEntry {
  moves: number[];
  length: number;
}

export const App: React.FC = () => {
  const [facelet, setFacelet] = useState<Int8Array>(() => createSolvedFacelet());
  // Default to "custom" (full visible) so users start with no constraints.
  const [mask, setMask] = useState<Mask>(() => createFullMask());
  const [presetId, setPresetId] = useState<string | null>(null);
  const [mode, setMode] = useState<EditorMode>("color");
  const [activeColor, setActiveColor] = useState<ColorT>(Color.U);
  const [dsl, setDsl] = useState<string>("<U, D, R, L, F, B>");
  const [moveInput, setMoveInput] = useState<string>("");
  const [moveError, setMoveError] = useState<string>("");
  // Defaults updated per plan v3 Phase R6 (INCIDENTS.md INC-2026-05-18-01):
  //   maxMoves 11 → 22 so a random scramble can actually be solved on first
  //     Solve press (random scramble HTM optimal is typically 17-22).
  //   outputMode shortest-only → enumerate-up-to-N so mask-heavy queries
  //     (OLL/PLL/F2L) stream multiple shortest solutions in CE incomplete
  //     cube filter style (QA #11).
  const [maxMoves, setMaxMoves] = useState<number>(22);
  const [outputMode, setOutputMode] = useState<OutputMode>("enumerate-up-to-N");
  const [topN, setTopN] = useState<number>(20);
  const [solutions, setSolutions] = useState<SolutionEntry[]>([]);
  const [progress, setProgress] = useState<string>("");
  const [running, setRunning] = useState<boolean>(false);
  const [jobId, setJobId] = useState<string | null>(null);
  // Pruning-table build state. While `tablesReady` is false the Start button
  // is disabled and a banner shows current build progress.
  const [tablesReady, setTablesReady] = useState<boolean>(false);
  const [buildProgress, setBuildProgress] = useState<string>("loading pruning tables…");

  // Empty DSL defaults to <R, U, F, D, L, B>. The actual string sent to the
  // worker is `effectiveDsl`.
  const effectiveDsl = dsl.trim() === "" ? "<R, U, F, D, L, B>" : dsl;

  const dslState = useMemo(() => parsePatternPreview(effectiveDsl), [effectiveDsl]);

  const validityError = useMemo(
    () => validateFacelet(facelet, mask) ?? "",
    [facelet, mask],
  );

  // Buffered solutions: flush in batches via requestAnimationFrame to avoid
  // O(N) setState calls per solution when the worker emits many.
  const pendingRef = useRef<SolutionEntry[]>([]);
  const rafRef = useRef<number | null>(null);
  const flushSolutions = (): void => {
    rafRef.current = null;
    if (pendingRef.current.length === 0) return;
    const batch = pendingRef.current;
    pendingRef.current = [];
    setSolutions((prev) => prev.concat(batch));
  };

  // Catch up on table-build events that may have fired before this listener
  // was registered (Tauri: cache-hit completes in ~110 ms, React mount is
  // slower, so the live events get lost without a snapshot query).
  useEffect(() => {
    if (!window.api?.getPruningStatus) return;
    window.api
      .getPruningStatus()
      .then((s) => {
        console.log("[pruning] snapshot", s);
        if (s.ready) {
          setTablesReady(true);
          setBuildProgress("");
        } else if (s.lastEvent && s.lastEvent.type === "table-build-start") {
          const human = s.lastEvent.source === "cache" ? "loading" : "building";
          setBuildProgress(
            `${human} pruning table ${s.lastEvent.step}/${s.lastEvent.total}: ${s.lastEvent.name}` +
              (s.lastEvent.source === "build" && s.lastEvent.name === "cp_co"
                ? " (~3 min, one-time)"
                : ""),
          );
        }
      })
      .catch((err: unknown) => {
        console.error("[pruning] getPruningStatus failed:", err);
        setBuildProgress(`pruning status query failed: ${err instanceof Error ? err.message : String(err)}`);
      });
  }, []);

  // Settings persistence (Tauri only — Electron path will skip silently).
  // Hold off on auto-saving until the initial load has run so we don't clobber
  // the saved file with default state during the first render.
  const settingsLoadedRef = useRef(false);
  useEffect(() => {
    const api = window.api as typeof window.api & {
      loadSettings?: () => Promise<string | null>;
    };
    if (!api?.loadSettings) {
      settingsLoadedRef.current = true;
      return;
    }
    void api.loadSettings().then((raw) => {
      if (raw) {
        try {
          const s = JSON.parse(raw) as {
            facelet?: number[];
            mask?: number[];
            presetId?: string | null;
            dsl?: string;
            maxMoves?: number;
            outputMode?: OutputMode;
            topN?: number;
            mode?: EditorMode;
          };
          if (Array.isArray(s.facelet) && s.facelet.length === 54) {
            setFacelet(Int8Array.from(s.facelet));
          }
          // Preset masks evolve across releases (e.g. CMLL gained U/D/F/B
          // centers). When a preset is selected, ALWAYS recompute the mask
          // from the current preset definition — never restore a stale mask
          // from settings. Custom masks (presetId === null) restore as-is.
          if (s.presetId) {
            setPresetId(s.presetId);
            const fresh = maskFromPresetId(s.presetId);
            if (fresh) setMask(fresh);
          } else {
            if (s.presetId === null) setPresetId(null);
            if (Array.isArray(s.mask) && s.mask.length === 54) {
              setMask(Uint8Array.from(s.mask));
            }
          }
          if (typeof s.dsl === "string") setDsl(s.dsl);
          if (typeof s.maxMoves === "number") setMaxMoves(s.maxMoves);
          if (s.outputMode) setOutputMode(s.outputMode);
          if (typeof s.topN === "number") setTopN(s.topN);
          if (s.mode === "color" || s.mode === "mask") setMode(s.mode);
        } catch {
          // Corrupt settings — ignore, will overwrite on next save.
        }
      }
      settingsLoadedRef.current = true;
    });
  }, []);

  useEffect(() => {
    if (!settingsLoadedRef.current) return;
    const api = window.api as typeof window.api & {
      saveSettings?: (json: string) => Promise<void>;
    };
    const saveSettings = api?.saveSettings;
    if (!saveSettings) return;
    const handle = setTimeout(() => {
      const json = JSON.stringify({
        facelet: Array.from(facelet),
        mask: Array.from(mask),
        presetId,
        dsl,
        maxMoves,
        outputMode,
        topN,
        mode,
      });
      void saveSettings(json).catch((e: unknown) => {
        console.error("saveSettings failed", e);
      });
    }, 500);
    return () => clearTimeout(handle);
  }, [facelet, mask, presetId, dsl, maxMoves, outputMode, topN, mode]);

  // Track the current jobId in a ref so the worker-event listener stays
  // registered once (no re-register on jobId change). Re-registering races
  // with the Tauri `listen` async path and drops the first `progress` event
  // — that's why Start used to stick on "starting…".
  const jobIdRef = useRef<string | null>(null);
  useEffect(() => {
    jobIdRef.current = jobId;
  }, [jobId]);

  useEffect(() => {
    if (!window.api) return;
    const off = window.api.onWorkerEvent((msg: FromWorker) => {
      // Table-build events don't carry jobId; always process them.
      if (msg.type === "table-build-start") {
        const human = msg.source === "cache" ? "loading" : "building";
        setBuildProgress(
          `${human} pruning table ${msg.step}/${msg.total}: ${msg.name}` +
            (msg.source === "build" && msg.name === "cp_co"
              ? " (~3 min, one-time)"
              : ""),
        );
        return;
      }
      if (msg.type === "table-build-step-done") {
        setBuildProgress(`pruning table ${msg.step}/${msg.total}: ${msg.name} ready (${msg.elapsedMs} ms)`);
        return;
      }
      if (msg.type === "table-build-all-done") {
        setTablesReady(true);
        setBuildProgress(`pruning tables ready (${msg.elapsedMs} ms)`);
        // Hide the banner shortly after.
        setTimeout(() => setBuildProgress(""), 3000);
        return;
      }
      const currentJobId = jobIdRef.current;
      if (currentJobId && "jobId" in msg && msg.jobId !== currentJobId) return;
      if (msg.type === "solution") {
        pendingRef.current.push({ moves: msg.moves, length: msg.length });
        if (rafRef.current === null) {
          rafRef.current = requestAnimationFrame(flushSolutions);
        }
      } else if (msg.type === "progress") {
        setProgress(
          `depth=${msg.depth} solutions=${msg.solutionsFound} nodes=${msg.nodesExplored} ${msg.elapsedMs}ms`,
        );
      } else if (msg.type === "complete") {
        // Flush any remaining buffered solutions before showing completion.
        if (pendingRef.current.length > 0) flushSolutions();
        // spec AC-imp-2: when 0 solutions and 0 nodes explored, the routing
        // proved unreachability via subgroup orbit BFS or PDB lower-bound
        // short-circuit — surface this explicitly so the user understands
        // "no solution exists" rather than "search ran but gave up".
        const provedUnreachable =
          msg.totalSolutions === 0 && msg.nodesExplored === 0;
        if (provedUnreachable) {
          setProgress(
            `到達不能と判定: 探索ノード 0、${msg.timeMs}ms (指定したマスク・MoveSet・最大手数では解が存在しない)`,
          );
        } else {
          setProgress(
            `done: ${msg.totalSolutions} solution(s), ${msg.timeMs}ms, ${msg.nodesExplored} nodes` +
              (msg.shortestLength !== null ? `, shortest=${msg.shortestLength}` : ""),
          );
        }
        setRunning(false);
      } else if (msg.type === "error") {
        setProgress(`error: ${msg.message}`);
        setRunning(false);
      }
    });
    return off;
  }, []);

  const handleApplyPreset = (id: string): void => {
    const p = presetById(id);
    if (!p) return;
    setPresetId(id);
    setMask(maskFromPresetId(id)!);
  };

  const handleFaceletChange = (index: number, newColor: ColorT): void => {
    setFacelet((prev) => {
      const out = new Int8Array(prev);
      out[index] = newColor;
      return out;
    });
    // If the sticker was masked (don't-care), paint also clears the mask so
    // the new color actually takes effect.
    setMask((prev) => {
      if (prev[index] === 1) return prev;
      const out = new Uint8Array(prev);
      out[index] = 1;
      return out;
    });
    setPresetId((prev) => {
      // Only clear preset id if we actually changed the mask.
      if (mask[index] === 1) return prev;
      return null;
    });
  };

  const handleMaskToggle = (index: number): void => {
    setMask((prev) => {
      const out = new Uint8Array(prev);
      out[index] = out[index] ? 0 : 1;
      return out;
    });
    setPresetId(null); // custom mask
  };

  const handleResetCube = (): void => setFacelet(createSolvedFacelet());

  const handleApplyMoves = (): void => {
    const text = moveInput.trim();
    if (text === "") {
      setMoveError("");
      return;
    }
    const tokens = text.split(/\s+/);
    const moves: number[] = [];
    for (const t of tokens) {
      const m = parseMoveToken(t);
      if (m === null) {
        setMoveError(`unknown move: "${t}"`);
        return;
      }
      moves.push(m);
    }
    setMoveError("");
    setFacelet((prev) => applyMoves(prev, moves));
    setMask((prev) => applyMovesToMask(prev, moves));
    setPresetId(null); // mask has moved → no longer a preset
    setMoveInput("");
  };

  const handleStart = (): void => {
    if (!dslState.ok) return;
    setSolutions([]);
    setProgress("starting...");
    setRunning(true);
    const id = String(Date.now());
    setJobId(id);
    window.api.sendSearch({
      type: "search-request",
      jobId: id,
      initial: Array.from(facelet),
      mask: Array.from(mask),
      dsl: effectiveDsl,
      maxMoves,
      outputMode,
      topN: outputMode === "top-n" ? topN : undefined,
    });
  };

  const handleCancel = (): void => {
    if (jobId) window.api.cancelSearch(jobId);
    // Optimistically mark the run as stopped so the UI updates immediately;
    // any in-flight worker events for this jobId after the terminate will be
    // ignored once `jobId` resets below.
    setRunning(false);
    setProgress("cancelled");
    setJobId(null);
  };

  const handleSave = async (): Promise<void> => {
    if (!window.api) return;
    const json = buildProjectJson({
      facelet,
      mask,
      preset: presetId,
      dsl,
      maxMoves,
      outputMode,
      topN: outputMode === "top-n" ? topN : undefined,
    });
    await window.api.saveProject("project.gencube.json", json);
  };

  const handleOpen = async (): Promise<void> => {
    if (!window.api) return;
    const res = await window.api.openProject();
    if (!res) return;
    try {
      const proj = parseProject(res.data);
      setFacelet(proj.facelet);
      setMask(proj.mask);
      setPresetId(proj.preset);
      setDsl(proj.dsl);
      setMaxMoves(proj.maxMoves);
      setOutputMode(proj.outputMode);
      setTopN(proj.topN ?? 20);
    } catch (e) {
      setProgress(`open error: ${e instanceof Error ? e.message : String(e)}`);
    }
  };

  const handleExportCsv = async (): Promise<void> => {
    const api = window.api as typeof window.api & {
      saveSolutionsCsv?: (defaultName: string, csv: string) => Promise<string | null>;
    };
    if (!api?.saveSolutionsCsv || solutions.length === 0) return;
    // Sort by length ascending so the CSV mirrors the on-screen order.
    const ordered = solutions.slice().sort((a, b) => a.length - b.length);
    const lines: string[] = ["length,moves"];
    for (const s of ordered) {
      // Moves are space-separated WCA tokens; no commas in the field, so a
      // bare CSV field is fine. Wrap in quotes anyway for tooling safety.
      lines.push(`${s.length},"${formatMoveSequence(s.moves)}"`);
    }
    const csv = lines.join("\r\n") + "\r\n";
    await api.saveSolutionsCsv("solutions.csv", csv);
  };

  const [showCqlHelp, setShowCqlHelp] = useState(false);

  return (
    <div className="app">
      <header className="app-header">
        <div className="app-brand">
          <span className="app-logo">⌬</span>
          <span className="app-name">CubeQL</span>
          <span className="app-tagline">Rubik&apos;s cube state search</span>
        </div>
        <div className="app-actions">
          <button onClick={handleOpen}>Open…</button>
          <button onClick={handleSave}>Save…</button>
        </div>
      </header>
      {buildProgress && (
        <div className="build-banner">
          {buildProgress}
        </div>
      )}
      <div className="left">
        <div className="panel">
          <div className="menu" style={{ display: "none" }}>
            <button onClick={handleOpen}>Open…</button>
            <button onClick={handleSave}>Save…</button>
            <button onClick={handleResetCube}>Reset cube</button>
          </div>
          <h2>Cube state &amp; mask</h2>
          <div className="toolbar">
            <div className="mode-toggle">
              <button
                className={mode === "color" ? "primary" : ""}
                onClick={() => setMode("color")}
              >
                Color
              </button>
              <button className={mode === "mask" ? "primary" : ""} onClick={() => setMode("mask")}>
                Mask
              </button>
            </div>
            {mode === "color" && (
              <>
                {(Object.values(Color) as ColorT[]).map((c) => (
                  <div
                    key={c}
                    className={`swatch color-${COLOR_NAMES[c]} ${activeColor === c ? "active" : ""}`}
                    onClick={() => setActiveColor(c)}
                    title={COLOR_NAMES[c]}
                  />
                ))}
              </>
            )}
          </div>
          <FaceletEditor
            facelet={facelet}
            mask={mask}
            mode={mode}
            activeColor={activeColor}
            onFaceletChange={handleFaceletChange}
            onMaskToggle={handleMaskToggle}
          />
          <div className="cube-actions">
            <button onClick={handleResetCube}>Reset cube</button>
          </div>
          <div style={{ color: "#909094", fontSize: 11, textAlign: "center", marginTop: 4 }}>
            {mode === "color"
              ? "Click a swatch above, then click stickers on the cube to paint them."
              : "Click any sticker to toggle ignored (hatched = don't-care)."}
          </div>
          <div className="row" style={{ marginTop: 8 }}>
            <input
              type="text"
              placeholder="apply moves, e.g. R U R' U'"
              value={moveInput}
              onChange={(e) => {
                setMoveInput(e.target.value);
                setMoveError("");
              }}
              onKeyDown={(e) => {
                if (e.key === "Enter") handleApplyMoves();
              }}
              style={{ flex: 1 }}
            />
            <button onClick={handleApplyMoves}>Apply</button>
          </div>
          {moveError && <div className="dsl-error">{moveError}</div>}
          {validityError && (
            <div className="dsl-error" style={{ marginTop: 4 }}>
              Cube state is not physical: {validityError}
            </div>
          )}
        </div>

        <div className="panel">
          <h2>Mask preset</h2>
          <div className="row">
            <select
              value={presetId ?? "(custom)"}
              onChange={(e) => {
                if (e.target.value === "(custom)") setPresetId(null);
                else handleApplyPreset(e.target.value);
              }}
            >
              <option value="(custom)">(custom)</option>
              {getPresets().map((p) => (
                <option key={p.id} value={p.id}>
                  {p.label}
                </option>
              ))}
            </select>
          </div>
        </div>
      </div>

      <div className="right">
        <div className="panel">
          <div className="panel-header">
            <h2>CubeQL — Cube Query Language</h2>
            <button
              className="ghost-button"
              onClick={() => setShowCqlHelp((v) => !v)}
              title="Show / hide CubeQL syntax help"
              aria-pressed={showCqlHelp}
            >
              {showCqlHelp ? "Hide help ✕" : "? Help"}
            </button>
          </div>
          <textarea
            rows={3}
            value={dsl}
            onChange={(e) => setDsl(e.target.value)}
            placeholder='e.g. <R, U>{1,7}  or  <U> F <R, U> F&apos; <U>'
          />
          {dslState.ok ? (
            <div className="dsl-preview">{dslState.preview}</div>
          ) : (
            <div className="dsl-error">
              line {dslState.error.line}, col {dslState.error.column}: {dslState.error.message}
            </div>
          )}
          {showCqlHelp && <CubeQLHelp />}
        </div>

        <div className="panel">
          <h2>Search</h2>
          <div className="row">
            <label>
              Max moves
              <input
                type="number"
                min={0}
                max={30}
                value={maxMoves}
                onChange={(e) => setMaxMoves(parseInt(e.target.value, 10) || 0)}
                style={{ width: 60 }}
              />
            </label>
            <label>
              <input
                type="radio"
                name="output"
                checked={outputMode === "shortest-only"}
                onChange={() => setOutputMode("shortest-only")}
              />
              Shortest only
            </label>
            <label>
              <input
                type="radio"
                name="output"
                checked={outputMode === "enumerate-up-to-N"}
                onChange={() => setOutputMode("enumerate-up-to-N")}
              />
              All ≤ N
            </label>
            <label>
              <input
                type="radio"
                name="output"
                checked={outputMode === "top-n"}
                onChange={() => setOutputMode("top-n")}
              />
              Top
              <input
                type="number"
                min={1}
                max={1000}
                value={topN}
                onChange={(e) => setTopN(parseInt(e.target.value, 10) || 1)}
                style={{ width: 50 }}
              />
            </label>
          </div>
          <div className="row">
            <button
              className="primary"
              onClick={handleStart}
              disabled={running || !dslState.ok || !!validityError || !tablesReady}
              title={
                !tablesReady
                  ? "Loading pruning tables…"
                  : validityError
                    ? `Cube not physical: ${validityError}`
                    : ""
              }
            >
              Start
            </button>
            <button onClick={handleCancel} disabled={!running}>
              Cancel
            </button>
            <span className="progress">{progress}</span>
          </div>
        </div>

        <div className="panel" style={{ flex: 1, minHeight: 0 }}>
          <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between" }}>
            <h2 style={{ margin: 0 }}>Solutions ({solutions.length})</h2>
            <button
              onClick={handleExportCsv}
              disabled={solutions.length === 0}
              title={solutions.length === 0 ? "No solutions to export" : "Save solutions as CSV"}
            >
              Export CSV…
            </button>
          </div>
          <div className="solutions" style={{ marginTop: 8 }}>
            {solutions
              .slice()
              .sort((a, b) => a.length - b.length)
              .slice(0, 500)
              .map((s, i) => (
              <div key={i} className="solution-row">
                <span className="len">{s.length}</span>
                <span className="moves">{formatMoveSequence(s.moves)}</span>
                <button
                  onClick={() => navigator.clipboard.writeText(formatMoveSequence(s.moves))}
                  style={{ padding: "2px 6px" }}
                >
                  Copy
                </button>
              </div>
            ))}
            {solutions.length > 500 && (
              <div style={{ color: "#909094", fontSize: 11, padding: "6px" }}>
                ... and {solutions.length - 500} more (display capped at 500)
              </div>
            )}
          </div>
        </div>
      </div>
    </div>
  );
};

