import { describe, expect, it } from "vite-plus/test";
import { newLayer } from "#lib/document/layers.js";
import type { DesignDocument, SceneObject } from "#lib/document/model.js";
import { IDENTITY, translate } from "#lib/document/transform.js";
import { benchmarkDocument, buildScene, MISSING_LAYER_COLOR, segmentCount } from "./scene.js";

const base = { name: "", layerId: "layer-1", transform: IDENTITY };
const doc = (...objects: SceneObject[]): DesignDocument => ({
  version: 1,
  units: "mm",
  layers: [newLayer(0)],
  objects,
});

describe("scene", () => {
  it("places objects in world space with their bounds", () => {
    const scene = buildScene(
      doc({
        ...base,
        id: "r",
        type: "rect",
        width: 10,
        height: 5,
        cornerRadius: 0,
        transform: translate(20, 30),
      }),
    );
    expect(scene.items).toHaveLength(1);
    expect(scene.items[0].bounds).toEqual({ minX: 20, minY: 30, maxX: 30, maxY: 35 });
    expect(scene.bounds).toEqual({ minX: 20, minY: 30, maxX: 30, maxY: 35 });
  });

  it("applies group transforms to children and reports the top-level id", () => {
    const scene = buildScene(
      doc({
        ...base,
        id: "g",
        type: "group",
        transform: translate(100, 0),
        children: [{ ...base, id: "e", type: "ellipse", rx: 5, ry: 2 }],
      }),
    );
    expect(scene.items[0].rootId).toBe("g");
    const { minX, maxX, minY, maxY } = scene.items[0].bounds;
    expect(minX).toBeCloseTo(95);
    expect(maxX).toBeCloseTo(105);
    expect(minY).toBeCloseTo(-2);
    expect(maxY).toBeCloseTo(2);
  });

  it("culls to the visible rectangle and hit-tests near strokes", () => {
    const scene = buildScene(
      doc(
        { ...base, id: "a", type: "rect", width: 10, height: 10, cornerRadius: 0 },
        {
          ...base,
          id: "b",
          type: "rect",
          width: 10,
          height: 10,
          cornerRadius: 0,
          transform: translate(100, 100),
        },
      ),
    );
    expect(scene.query({ minX: 90, minY: 90, maxX: 200, maxY: 200 }).map((i) => i.id)).toEqual([
      "b",
    ]);
    expect(scene.hitTest({ x: 10.2, y: 5 }, 0.5)).toBe("a");
    // Inside the outline but away from it: unfilled outlines are not hit.
    expect(scene.hitTest({ x: 5, y: 5 }, 0.5)).toBeNull();
    expect(scene.hitTest({ x: 50, y: 50 }, 0.5)).toBeNull();
  });

  it("hit-tests the topmost object first", () => {
    const square = { ...base, type: "rect" as const, width: 10, height: 10, cornerRadius: 0 };
    const scene = buildScene(doc({ ...square, id: "under" }, { ...square, id: "over" }));
    expect(scene.hitTest({ x: 0, y: 5 }, 0.5)).toBe("over");
  });

  it("draws objects in their layer's colour and leaves hidden layers out", () => {
    const square = { ...base, type: "rect" as const, width: 10, height: 10, cornerRadius: 0 };
    const red = { ...newLayer(1), id: "red" };
    const shapes = doc(
      { ...square, id: "a" },
      { ...square, id: "b", layerId: "red" },
      { ...square, id: "c", layerId: "gone" },
    );
    const scene = buildScene({ ...shapes, layers: [newLayer(0), red] });
    expect(scene.items.map((item) => [item.id, item.color])).toEqual([
      ["a", "#0000ff"],
      ["b", "#ff0000"],
      ["c", MISSING_LAYER_COLOR],
    ]);
    const hidden = buildScene({ ...shapes, layers: [newLayer(0), { ...red, shown: false }] });
    expect(hidden.items.map((item) => item.id)).toEqual(["a", "c"]);
    expect(hidden.hitTest({ x: 0, y: 5 }, 0.5)).toBe("c");
  });

  it("hits text anywhere in its box and leaves out empty text", () => {
    const glyph = (x: number) => ({
      start: { x, y: 0 },
      segments: [
        { kind: "line" as const, to: { x: x + 1, y: 0 } },
        { kind: "line" as const, to: { x: x + 1, y: 10 } },
      ],
      closed: true,
    });
    const text = {
      ...base,
      type: "text" as const,
      text: "II",
      fontFamily: "Arial",
      fontWeight: 400,
      italic: false,
      height: 10,
      letterSpacing: 0,
      lineSpacing: 1,
      align: "left" as const,
    };
    const scene = buildScene(
      doc(
        { ...text, id: "t", outline: [glyph(0), glyph(8)] },
        { ...text, id: "empty", text: "", outline: [] },
      ),
    );
    expect(scene.items.map((item) => item.id)).toEqual(["t"]);
    // Between the two letters, far from either outline.
    expect(scene.hitTest({ x: 4.5, y: 5 }, 0.5)).toBe("t");
    expect(scene.hitTest({ x: 20, y: 5 }, 0.5)).toBeNull();
  });

  it("marks shapes on Fill layers, but not on Line or missing layers", () => {
    const square = { ...base, type: "rect" as const, width: 10, height: 10, cornerRadius: 0 };
    const fill = { ...newLayer(1), id: "fill", mode: "fill" as const };
    const scene = buildScene({
      ...doc(
        { ...square, id: "line" },
        { ...square, id: "filled", layerId: "fill" },
        { ...square, id: "orphan", layerId: "gone" },
      ),
      layers: [newLayer(0), fill],
    });
    expect(scene.items.map((item) => [item.id, item.fill])).toEqual([
      ["line", false],
      ["filled", true],
      ["orphan", false],
    ]);
  });

  it("builds a benchmark document with the requested segment count", () => {
    const scene = buildScene(benchmarkDocument(10_000));
    expect(segmentCount(scene)).toBe(10_000);
  });
});
