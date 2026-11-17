import type { Bounds } from "./view.js";

/** The machine's work area in millimetres, with the origin at the bottom-left corner. */
export interface Bed {
  width: number;
  height: number;
}

/**
 * Used until machine profiles land; then the canvas takes the active
 * profile's bed instead. 400 × 400 mm matches the Elegoo Phecda.
 */
export const DEFAULT_BED: Bed = { width: 400, height: 400 };

export function bedBounds(bed: Bed): Bounds {
  return { minX: 0, minY: 0, maxX: bed.width, maxY: bed.height };
}
