/**
 * Material test grids in the document: adding a generated grid, and finding
 * its cells again so one can be saved as a material preset. The grid itself
 * is laid out in Rust (`bf_core::material_test`), which names the layers
 * `<grid id>-labels` and `<grid id>-r<row>-c<column>`.
 */
import type { TestGrid } from "#lib/bindings/test-grid/TestGrid.js";
import type { TestGridSettings } from "#lib/bindings/test-grid/TestGridSettings.js";
import { addLayer, batch, insert, type Command } from "#lib/document/commands.js";
import { layerUsage } from "#lib/document/layers.js";
import type { CutLayer, DesignDocument } from "#lib/document/model.js";

export type { TestGrid } from "#lib/bindings/test-grid/TestGrid.js";
export type { TestGridSettings } from "#lib/bindings/test-grid/TestGridSettings.js";

/**
 * A 5 × 5 engrave test for a 20 W diode: 1000–3000 mm/min down the rows,
 * 20–100% across the columns, 10 mm cells, labels scored at 10%. The same
 * as `TestGridSettings::default()` in Rust.
 */
export function defaultTestGridSettings(fontFamily = "Arial"): TestGridSettings {
  return {
    speed: { from: 1000, to: 3000, steps: 5 },
    power: { from: 20, to: 100, steps: 5 },
    cellSize: 10,
    gap: 2,
    mode: "fill",
    lineInterval: 0.1,
    passes: 1,
    airAssist: true,
    labels: { fontFamily, height: 3, speed: 3000, power: 10 },
    origin: { x: 10, y: 10 },
  };
}

/** The command that adds `grid`'s layers (cut after the document's own) and its group. */
export function addTestGrid(grid: TestGrid): Command {
  return batch([...grid.layers.map((layer) => addLayer(layer)), insert(grid.objects)]);
}

/** A grid's cells in the document, by row (speed) and column (power). */
export interface TestGridCells {
  id: string;
  rows: number;
  columns: number;
  /** `cells[row][column]`: the cell's layer, or null if it has gone. */
  cells: (CutLayer | null)[][];
}

const CELL_ID = /^(.+)-r(\d+)-c(\d+)$/;

/**
 * The most recently added test grid that still has cells on the canvas, or
 * null. Cells whose layer has nothing on it any more are left out.
 */
export function findTestGrid(doc: DesignDocument): TestGridCells | null {
  const used = layerUsage(doc.objects);
  const grids = new Map<string, { row: number; column: number; layer: CutLayer }[]>();
  for (const layer of doc.layers) {
    const match = CELL_ID.exec(layer.id);
    if (!match || !used.has(layer.id)) continue;
    const [, id, row, column] = match;
    if (!doc.layers.some((other) => other.id === `${id}-labels`)) continue;
    const cells = grids.get(id) ?? [];
    cells.push({ row: Number(row), column: Number(column), layer });
    grids.set(id, cells);
  }
  const latest = [...grids.entries()].at(-1);
  if (!latest) return null;
  const [id, found] = latest;
  const rows = Math.max(...found.map((cell) => cell.row)) + 1;
  const columns = Math.max(...found.map((cell) => cell.column)) + 1;
  const cells = Array.from({ length: rows }, () => Array<CutLayer | null>(columns).fill(null));
  for (const { row, column, layer } of found) cells[row][column] = layer;
  return { id, rows, columns, cells };
}
