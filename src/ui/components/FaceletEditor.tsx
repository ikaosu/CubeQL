/**
 * 2D unfolded facelet editor.
 *
 * Layout (each cell = 1 grid square; the grid is 12 cols × 9 rows):
 *               U U U
 *               U U U
 *               U U U
 *   L L L F F F R R R B B B
 *   L L L F F F R R R B B B
 *   L L L F F F R R R B B B
 *               D D D
 *               D D D
 *               D D D
 *
 * Each cell either represents one facelet (clickable) or is empty (filler).
 */

import React from "react";
import { COLOR_NAMES, type Color } from "../cube-engine";

type CellSpec =
  | { kind: "empty" }
  | { kind: "facelet"; index: number };

function buildGrid(): CellSpec[] {
  // 12 cols × 9 rows = 108 cells. We'll fill in the cube layout.
  const cells: CellSpec[] = new Array(108).fill(null).map(() => ({ kind: "empty" }));

  // U face at rows 0-2, cols 3-5. Index 0..8 row-major.
  for (let r = 0; r < 3; r++) {
    for (let c = 0; c < 3; c++) {
      cells[r * 12 + (3 + c)] = { kind: "facelet", index: r * 3 + c };
    }
  }
  // L face at rows 3-5, cols 0-2. Index 36..44.
  for (let r = 0; r < 3; r++) {
    for (let c = 0; c < 3; c++) {
      cells[(3 + r) * 12 + c] = { kind: "facelet", index: 36 + r * 3 + c };
    }
  }
  // F face at rows 3-5, cols 3-5. Index 18..26.
  for (let r = 0; r < 3; r++) {
    for (let c = 0; c < 3; c++) {
      cells[(3 + r) * 12 + (3 + c)] = { kind: "facelet", index: 18 + r * 3 + c };
    }
  }
  // R face at rows 3-5, cols 6-8. Index 9..17.
  for (let r = 0; r < 3; r++) {
    for (let c = 0; c < 3; c++) {
      cells[(3 + r) * 12 + (6 + c)] = { kind: "facelet", index: 9 + r * 3 + c };
    }
  }
  // B face at rows 3-5, cols 9-11. Index 45..53.
  for (let r = 0; r < 3; r++) {
    for (let c = 0; c < 3; c++) {
      cells[(3 + r) * 12 + (9 + c)] = { kind: "facelet", index: 45 + r * 3 + c };
    }
  }
  // D face at rows 6-8, cols 3-5. Index 27..35.
  for (let r = 0; r < 3; r++) {
    for (let c = 0; c < 3; c++) {
      cells[(6 + r) * 12 + (3 + c)] = { kind: "facelet", index: 27 + r * 3 + c };
    }
  }
  return cells;
}

const GRID = buildGrid();

export interface FaceletEditorProps {
  facelet: Int8Array;
  mask?: Uint8Array;
  /** Active mode: 'color' edits sticker color, 'mask' toggles don't-care. */
  mode: "color" | "mask";
  /** Active color when mode is 'color'. */
  activeColor: Color;
  onFaceletChange: (index: number, newColor: Color) => void;
  onMaskToggle?: (index: number) => void;
}

export const FaceletEditor: React.FC<FaceletEditorProps> = ({
  facelet,
  mask,
  mode,
  activeColor,
  onFaceletChange,
  onMaskToggle,
}) => {
  const handleClick = (index: number): void => {
    if (mode === "color") {
      onFaceletChange(index, activeColor);
    } else if (onMaskToggle) {
      onMaskToggle(index);
    }
  };

  return (
    <div className="facelet">
      {GRID.map((cell, i) => {
        if (cell.kind === "empty") {
          return <div key={i} className="cell cell-empty" />;
        }
        const idx = cell.index;
        const color = facelet[idx];
        const isDontCare = mask !== undefined && mask[idx] === 0;
        // Masked stickers: show no color (gray placeholder) — they carry no
        // colour value under traveling-wildcard semantics.
        const colorClass = isDontCare ? "color-masked" : `color-${COLOR_NAMES[color]}`;
        return (
          <div
            key={i}
            className={`cell ${colorClass} ${isDontCare ? "dontcare" : ""}`}
            onClick={() => handleClick(idx)}
            title={
              isDontCare
                ? `facelet[${idx}] (masked: wildcard, no colour)`
                : `facelet[${idx}] = ${COLOR_NAMES[color]}`
            }
          />
        );
      })}
    </div>
  );
};
