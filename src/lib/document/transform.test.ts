import { describe, expect, it } from "vite-plus/test";
import { applyToPoint, IDENTITY, multiply, rotate, scale, translate } from "./transform.js";

describe("affine transforms", () => {
  it("translates, scales and rotates points", () => {
    expect(applyToPoint(translate(2, 3), { x: 1, y: 1 })).toEqual({ x: 3, y: 4 });
    expect(applyToPoint(scale(2, -1), { x: 1, y: 1 })).toEqual({ x: 2, y: -1 });
    const turned = applyToPoint(rotate(Math.PI / 2), { x: 1, y: 0 });
    expect(turned.x).toBeCloseTo(0);
    expect(turned.y).toBeCloseTo(1);
  });

  it("composes right to left: multiply(a, b) applies b first", () => {
    const moved = multiply(translate(10, 0), scale(2, 2));
    expect(applyToPoint(moved, { x: 1, y: 1 })).toEqual({ x: 12, y: 2 });
    expect(multiply(IDENTITY, moved)).toEqual(moved);
  });
});
