import type { Bounds } from "./view.js";

/** The machine's work area in millimetres, with the origin at the bottom-left corner. */
export interface Bed {
  width: number;
  height: number;
}

export function bedBounds(bed: Bed): Bounds {
  return { minX: 0, minY: 0, maxX: bed.width, maxY: bed.height };
}
