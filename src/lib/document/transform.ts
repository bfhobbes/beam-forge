import type { Point } from "./model.js";

/**
 * A 2D affine transform `[a, b, c, d, e, f]` in SVG/canvas order:
 * x' = a·x + c·y + e, y' = b·x + d·y + f. Units are millimetres.
 */
export type Transform = readonly [number, number, number, number, number, number];

export const IDENTITY: Transform = [1, 0, 0, 1, 0, 0];

export function translate(x: number, y: number): Transform {
  return [1, 0, 0, 1, x, y];
}

export function scale(x: number, y = x): Transform {
  return [x, 0, 0, y, 0, 0];
}

/** Counter-clockwise rotation in radians (Y is up, matching the machine). */
export function rotate(radians: number): Transform {
  const cos = Math.cos(radians);
  const sin = Math.sin(radians);
  return [cos, sin, -sin, cos, 0, 0];
}

/** The transform that applies `inner` first, then `outer`. */
export function multiply(outer: Transform, inner: Transform): Transform {
  const [a1, b1, c1, d1, e1, f1] = outer;
  const [a2, b2, c2, d2, e2, f2] = inner;
  return [
    a1 * a2 + c1 * b2,
    b1 * a2 + d1 * b2,
    a1 * c2 + c1 * d2,
    b1 * c2 + d1 * d2,
    a1 * e2 + c1 * f2 + e1,
    b1 * e2 + d1 * f2 + f1,
  ];
}

export function applyToPoint([a, b, c, d, e, f]: Transform, { x, y }: Point): Point {
  return { x: a * x + c * y + e, y: b * x + d * y + f };
}
