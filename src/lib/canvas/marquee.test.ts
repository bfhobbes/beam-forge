import { describe, expect, it } from "vite-plus/test";
import type { DesignDocument, SceneObject } from "#lib/document/model.js";
import { IDENTITY, translate } from "#lib/document/transform.js";
import { marqueeMode, marqueeSelect } from "./marquee.js";
import { buildScene } from "./scene.js";

const rect = (id: string, x: number, y: number): SceneObject => ({
  type: "rect",
  id,
  name: id,
  layerId: "layer-1",
  transform: translate(x, y),
  width: 10,
  height: 10,
  cornerRadius: 0,
});
const doc = (...objects: SceneObject[]): DesignDocument => ({
  version: 1,
  units: "mm",
  layers: [],
  objects,
});

describe("marquee", () => {
  it("is a window when dragged left to right and crossing when right to left", () => {
    expect(marqueeMode({ x: 10, y: 50 }, { x: 80, y: 0 })).toBe("window");
    expect(marqueeMode({ x: 80, y: 0 }, { x: 10, y: 50 })).toBe("crossing");
  });

  const scene = buildScene(
    doc(
      rect("inside", 10, 10),
      rect("straddling", 25, 10),
      rect("outside", 100, 100),
      // A group selects as a whole, by its top-level id.
      {
        type: "group",
        id: "group",
        name: "group",
        layerId: "layer-1",
        transform: IDENTITY,
        children: [rect("g1", 10, 40), rect("g2", 60, 40)],
      },
    ),
  );
  const box = { minX: 0, minY: 0, maxX: 30, maxY: 55 };

  it("window selects only objects entirely inside", () => {
    expect(marqueeSelect(scene, box, "window")).toEqual(["inside"]);
  });

  it("crossing selects objects the box touches, in paint order", () => {
    expect(marqueeSelect(scene, box, "crossing")).toEqual(["inside", "straddling", "group"]);
  });

  it("crossing ignores outlines whose bounds overlap but whose lines miss", () => {
    // Inside the hollow middle of a 10 mm square, touching no edge.
    expect(marqueeSelect(scene, { minX: 12, minY: 12, maxX: 18, maxY: 18 }, "crossing")).toEqual(
      [],
    );
    // Crossing one edge with no vertex inside the box.
    expect(marqueeSelect(scene, { minX: 12, minY: 5, maxX: 18, maxY: 12 }, "crossing")).toEqual([
      "inside",
    ]);
  });
});
