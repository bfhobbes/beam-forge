import { describe, expect, it } from "vite-plus/test";
import {
  fitBounds,
  gridSteps,
  MAX_ZOOM,
  MIN_ZOOM,
  panBy,
  rulerStep,
  screenToWorld,
  visibleWorld,
  wheelGesture,
  worldToScreen,
  zoomAt,
  type View,
} from "./view.js";

const view: View = { zoom: 2, x: 100, y: 500 };

describe("screen and world transforms", () => {
  it("maps world mm to screen px with Y flipped", () => {
    expect(worldToScreen(view, { x: 0, y: 0 })).toEqual({ x: 100, y: 500 });
    expect(worldToScreen(view, { x: 10, y: 20 })).toEqual({ x: 120, y: 460 });
  });

  it("round-trips through screenToWorld", () => {
    const world = { x: 12.5, y: -3.25 };
    expect(screenToWorld(view, worldToScreen(view, world))).toEqual(world);
  });

  it("reports the world rectangle a viewport shows", () => {
    expect(visibleWorld(view, { width: 200, height: 100 })).toEqual({
      minX: -50,
      minY: 200,
      maxX: 50,
      maxY: 250,
    });
  });
});

describe("zoom and pan", () => {
  it("keeps the world point under the cursor fixed", () => {
    const cursor = { x: 300, y: 200 };
    const before = screenToWorld(view, cursor);
    const zoomed = zoomAt(view, cursor, 3);
    expect(zoomed.zoom).toBe(6);
    const after = screenToWorld(zoomed, cursor);
    expect(after.x).toBeCloseTo(before.x);
    expect(after.y).toBeCloseTo(before.y);
  });

  it("clamps zoom and stays put at the limits", () => {
    expect(zoomAt(view, { x: 0, y: 0 }, 1e9).zoom).toBe(MAX_ZOOM);
    expect(zoomAt(view, { x: 0, y: 0 }, 1e-9).zoom).toBe(MIN_ZOOM);
    const top = { ...view, zoom: MAX_ZOOM };
    expect(zoomAt(top, { x: 40, y: 40 }, 2)).toEqual(top);
  });

  it("pans by screen pixels", () => {
    expect(panBy(view, 5, -7)).toEqual({ zoom: 2, x: 105, y: 493 });
  });
});

describe("fit", () => {
  it("centres the bounds in the viewport with padding", () => {
    const fitted = fitBounds(
      { minX: 0, minY: 0, maxX: 400, maxY: 400 },
      { x: 0, y: 0, width: 1000, height: 500 },
      50,
    );
    expect(fitted.zoom).toBe(1);
    expect(worldToScreen(fitted, { x: 200, y: 200 })).toEqual({ x: 500, y: 250 });
    expect(worldToScreen(fitted, { x: 0, y: 400 })).toEqual({ x: 300, y: 50 });
  });

  it("fits within an inset viewport, e.g. beside rulers", () => {
    const fitted = fitBounds(
      { minX: 0, minY: 0, maxX: 100, maxY: 100 },
      { x: 20, y: 20, width: 200, height: 200 },
      0,
    );
    expect(fitted.zoom).toBe(2);
    expect(worldToScreen(fitted, { x: 0, y: 100 })).toEqual({ x: 20, y: 20 });
  });

  it("does not blow up on a zero-size box", () => {
    const fitted = fitBounds(
      { minX: 5, minY: 5, maxX: 5, maxY: 5 },
      { x: 0, y: 0, width: 100, height: 100 },
      0,
    );
    expect(fitted.zoom).toBe(MAX_ZOOM);
    expect(worldToScreen(fitted, { x: 5, y: 5 })).toEqual({ x: 50, y: 50 });
  });
});

describe("grid and rulers", () => {
  it("picks a 1-2-5 minor step at least the minimum pixel spacing", () => {
    expect(gridSteps(10, 8)).toEqual({ minor: 1, major: 10 });
    expect(gridSteps(3, 8)).toEqual({ minor: 5, major: 10 });
    expect(gridSteps(1, 8)).toEqual({ minor: 10, major: 100 });
    expect(gridSteps(0.5, 8)).toEqual({ minor: 20, major: 100 });
    expect(gridSteps(100, 8).minor).toBeCloseTo(0.1);
  });

  it("spaces ruler labels far enough apart to read", () => {
    expect(rulerStep(1, 60)).toBe(100);
    expect(rulerStep(10, 60)).toBe(10);
    expect(rulerStep(4, 60)).toBe(20);
  });
});

describe("wheel gestures", () => {
  const wheel = (over: Partial<Parameters<typeof wheelGesture>[0]>) =>
    wheelGesture({ deltaX: 0, deltaY: 0, deltaMode: 0, ctrlKey: false, shiftKey: false, ...over });

  it("zooms on a trackpad pinch (ctrl+wheel)", () => {
    const gesture = wheel({ ctrlKey: true, deltaY: 10 });
    expect(gesture.kind).toBe("zoom");
    if (gesture.kind === "zoom") expect(gesture.factor).toBeLessThan(1);
  });

  it("zooms on a mouse wheel notch, in when scrolling up", () => {
    const gesture = wheel({ deltaY: -100 });
    expect(gesture.kind).toBe("zoom");
    if (gesture.kind === "zoom") expect(gesture.factor).toBeGreaterThan(1);
    expect(wheel({ deltaY: 3, deltaMode: 1 }).kind).toBe("zoom");
  });

  it("pans on a trackpad two-finger scroll", () => {
    expect(wheel({ deltaX: 4, deltaY: 2 })).toEqual({ kind: "pan", dx: -4, dy: -2 });
    expect(wheel({ deltaY: 7.5 })).toEqual({ kind: "pan", dx: 0, dy: -7.5 });
  });

  it("pans sideways on shift+wheel", () => {
    expect(wheel({ shiftKey: true, deltaY: 100 })).toEqual({ kind: "pan", dx: -100, dy: 0 });
  });
});
