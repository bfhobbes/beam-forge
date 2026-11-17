import { describe, expect, it } from "vite-plus/test";
import { applyCommand } from "#lib/document/commands.js";
import { findPlacement, type DesignDocument, type SceneObject } from "#lib/document/model.js";
import {
  applyToPoint,
  IDENTITY,
  rotate,
  translate,
  type Transform,
} from "#lib/document/transform.js";
import {
  anchorPoint,
  dragTransform,
  propertyTransform,
  readProperties,
  selectionBounds,
  transformObjects,
} from "./arrange.js";
import type { Bounds } from "./view.js";

const rect = (id: string, x: number, y: number, width = 10, height = 5): SceneObject => ({
  type: "rect",
  id,
  name: id,
  layerId: "layer-1",
  transform: translate(x, y),
  width,
  height,
  cornerRadius: 0,
});
const doc = (...objects: SceneObject[]): DesignDocument => ({
  version: 1,
  units: "mm",
  layers: [],
  objects,
});
const box: Bounds = { minX: 10, minY: 20, maxX: 50, maxY: 40 };
const none = { shift: false, alt: false };

function expectPoint(actual: { x: number; y: number }, x: number, y: number) {
  expect(actual.x).toBeCloseTo(x, 9);
  expect(actual.y).toBeCloseTo(y, 9);
}

function expectBounds(actual: Bounds | null, expected: Bounds) {
  expect(actual).not.toBeNull();
  for (const key of ["minX", "minY", "maxX", "maxY"] as const)
    expect(actual![key]).toBeCloseTo(expected[key], 6);
}

/** Applies `by` to every object in `ids` and returns the result. */
function apply(document: DesignDocument, ids: string[], by: Transform): DesignDocument {
  return applyCommand(document, transformObjects(document, ids, by)).doc;
}

describe("anchors", () => {
  it("maps the nine anchor points onto a box, Y up", () => {
    expectPoint(anchorPoint(box, { x: 0, y: 0 }), 10, 20);
    expectPoint(anchorPoint(box, { x: 0.5, y: 0.5 }), 30, 30);
    expectPoint(anchorPoint(box, { x: 1, y: 1 }), 50, 40);
    expectPoint(anchorPoint(box, { x: 0, y: 1 }), 10, 40);
  });
});

describe("selection bounds", () => {
  it("unions the selected top-level objects only", () => {
    const d = doc(rect("a", 0, 0), rect("b", 100, 50), rect("c", 500, 500));
    expect(selectionBounds(d, ["a", "b"])).toEqual({ minX: 0, minY: 0, maxX: 110, maxY: 55 });
    expect(selectionBounds(d, [])).toBeNull();
  });
});

describe("transforming objects", () => {
  it("applies a world transform on top of each object's own", () => {
    const d = doc(rect("a", 5, 5));
    const command = transformObjects(d, ["a"], translate(10, 0));
    expect(command).toEqual({ type: "setTransforms", transforms: { a: translate(15, 5) } });
  });

  it("starts from the given original transforms during a drag", () => {
    const d = doc(rect("a", 5, 5));
    const command = transformObjects(d, ["a"], translate(1, 2), { a: IDENTITY });
    expect(command).toEqual({ type: "setTransforms", transforms: { a: translate(1, 2) } });
  });
});

describe("drag transforms", () => {
  it("moves by the pointer delta", () => {
    const t = dragTransform({ kind: "move" }, box, { x: 0, y: 0 }, { x: 3, y: -4 }, none);
    expectPoint(applyToPoint(t, { x: 10, y: 20 }), 13, 16);
  });

  it("scales from the opposite corner", () => {
    // Drag the top-right corner 40 mm right: width doubles, bottom-left stays put.
    const t = dragTransform(
      { kind: "scale", handle: "ne" },
      box,
      { x: 50, y: 40 },
      { x: 90, y: 40 },
      none,
    );
    expectPoint(applyToPoint(t, { x: 10, y: 20 }), 10, 20);
    expectPoint(applyToPoint(t, { x: 50, y: 40 }), 90, 40);
  });

  it("scales one axis from an edge", () => {
    const t = dragTransform(
      { kind: "scale", handle: "s" },
      box,
      { x: 30, y: 20 },
      { x: 99, y: 10 },
      none,
    );
    // The top edge stays; the bottom edge follows the pointer's Y only.
    expectPoint(applyToPoint(t, { x: 10, y: 40 }), 10, 40);
    expectPoint(applyToPoint(t, { x: 50, y: 20 }), 50, 10);
  });

  it("keeps the aspect ratio with shift", () => {
    const t = dragTransform(
      { kind: "scale", handle: "ne" },
      box,
      { x: 50, y: 40 },
      { x: 90, y: 41 },
      { shift: true, alt: false },
    );
    // Width doubles, so height doubles too.
    expectPoint(applyToPoint(t, { x: 50, y: 40 }), 90, 60);
    expectPoint(applyToPoint(t, { x: 10, y: 20 }), 10, 20);
  });

  it("keeps the aspect ratio from an edge with shift, about the edge's middle", () => {
    const t = dragTransform(
      { kind: "scale", handle: "e" },
      box,
      { x: 50, y: 30 },
      { x: 90, y: 30 },
      { shift: true, alt: false },
    );
    expectPoint(applyToPoint(t, { x: 10, y: 30 }), 10, 30);
    expectPoint(applyToPoint(t, { x: 50, y: 40 }), 90, 50);
  });

  it("scales from the centre with alt", () => {
    const t = dragTransform(
      { kind: "scale", handle: "e" },
      box,
      { x: 50, y: 30 },
      { x: 70, y: 30 },
      { shift: false, alt: true },
    );
    expectPoint(applyToPoint(t, { x: 30, y: 30 }), 30, 30);
    expectPoint(applyToPoint(t, { x: 50, y: 30 }), 70, 30);
    expectPoint(applyToPoint(t, { x: 10, y: 30 }), -10, 30);
  });

  it("does not collapse a box with no height", () => {
    const flat: Bounds = { minX: 0, minY: 0, maxX: 10, maxY: 0 };
    const t = dragTransform(
      { kind: "scale", handle: "ne" },
      flat,
      { x: 10, y: 0 },
      { x: 20, y: 5 },
      none,
    );
    expectPoint(applyToPoint(t, { x: 10, y: 0 }), 20, 0);
    expect(Number.isFinite(t[3])).toBe(true);
  });

  it("rotates about the centre by the pointer's sweep", () => {
    // From due east of the centre to due north: a quarter turn anticlockwise.
    const t = dragTransform({ kind: "rotate" }, box, { x: 60, y: 30 }, { x: 30, y: 70 }, none);
    expectPoint(applyToPoint(t, { x: 30, y: 30 }), 30, 30);
    expectPoint(applyToPoint(t, { x: 50, y: 30 }), 30, 50);
  });

  it("snaps rotation to 15 degree steps with shift", () => {
    const sweep = (20 * Math.PI) / 180;
    const to = { x: 30 + 30 * Math.cos(sweep), y: 30 + 30 * Math.sin(sweep) };
    const t = dragTransform({ kind: "rotate" }, box, { x: 60, y: 30 }, to, {
      shift: true,
      alt: false,
    });
    const step = (15 * Math.PI) / 180;
    expect(Math.atan2(t[1], t[0])).toBeCloseTo(step, 9);
  });
});

describe("numeric properties", () => {
  const d = doc(rect("a", 10, 20, 40, 20));
  const bounds = selectionBounds(d, ["a"])!;

  it("reads position at the anchor, size and rotation", () => {
    expect(readProperties(d, ["a"], bounds, { x: 0, y: 0 })).toEqual({
      x: 10,
      y: 20,
      width: 40,
      height: 20,
      rotation: 0,
    });
    const centre = readProperties(d, ["a"], bounds, { x: 0.5, y: 0.5 });
    expect([centre.x, centre.y]).toEqual([30, 30]);
  });

  it("reads a single object's rotation in degrees, anticlockwise", () => {
    const turned = doc({ ...rect("a", 0, 0), transform: rotate(Math.PI / 6) });
    const props = readProperties(turned, ["a"], selectionBounds(turned, ["a"])!, {
      x: 0.5,
      y: 0.5,
    });
    expect(props.rotation).toBeCloseTo(30, 9);
  });

  it("moves so the anchor lands on the new position", () => {
    const anchor = { x: 1, y: 1 } as const;
    const props = readProperties(d, ["a"], bounds, anchor);
    const t = propertyTransform(props, bounds, anchor, "x", 100)!;
    const moved = apply(d, ["a"], t);
    expect(readProperties(moved, ["a"], selectionBounds(moved, ["a"])!, anchor).x).toBeCloseTo(
      100,
      9,
    );
  });

  it("resizes about the anchor", () => {
    const anchor = { x: 1, y: 0.5 } as const;
    const props = readProperties(d, ["a"], bounds, anchor);
    const t = propertyTransform(props, bounds, anchor, "width", 120 / 2)!;
    expectBounds(selectionBounds(apply(d, ["a"], t), ["a"]), {
      minX: -10,
      minY: 20,
      maxX: 50,
      maxY: 40,
    });
  });

  it("refuses sizes that would collapse or flip the selection", () => {
    const props = readProperties(d, ["a"], bounds, { x: 0, y: 0 });
    expect(propertyTransform(props, bounds, { x: 0, y: 0 }, "width", 0)).toBeNull();
    expect(propertyTransform(props, bounds, { x: 0, y: 0 }, "height", -5)).toBeNull();
  });

  it("rotates to the new angle about the anchor", () => {
    const anchor = { x: 0, y: 0 } as const;
    const props = readProperties(d, ["a"], bounds, anchor);
    const t = propertyTransform(props, bounds, anchor, "rotation", 90)!;
    const turned = apply(d, ["a"], t);
    const after = findPlacement(turned.objects, "a")!.object.transform;
    expect(Math.atan2(after[1], after[0])).toBeCloseTo(Math.PI / 2, 9);
    expectBounds(selectionBounds(turned, ["a"]), { minX: -10, minY: 20, maxX: 10, maxY: 60 });
  });

  it("gives the same result as the matching drag on the canvas", () => {
    // Dragging the right edge 20 mm right == typing W + 20 with a left anchor.
    const drag = dragTransform(
      { kind: "scale", handle: "e" },
      bounds,
      { x: 50, y: 30 },
      { x: 70, y: 30 },
      none,
    );
    const anchor = { x: 0, y: 0.5 } as const;
    const props = readProperties(d, ["a"], bounds, anchor);
    const typed = propertyTransform(props, bounds, anchor, "width", props.width + 20)!;
    expect(apply(d, ["a"], typed)).toEqual(apply(d, ["a"], drag));

    // Rotating 90° with the centre handle == typing rotation 90 with a centre anchor.
    const spin = dragTransform(
      { kind: "rotate" },
      bounds,
      { x: 60, y: 30 },
      { x: 30, y: 60 },
      none,
    );
    const centre = { x: 0.5, y: 0.5 } as const;
    const fromCentre = readProperties(d, ["a"], bounds, centre);
    const rotated = propertyTransform(fromCentre, bounds, centre, "rotation", 90)!;
    const a = findPlacement(apply(d, ["a"], rotated).objects, "a")!.object.transform;
    const b = findPlacement(apply(d, ["a"], spin).objects, "a")!.object.transform;
    for (let i = 0; i < 6; i++) expect(a[i]).toBeCloseTo(b[i], 9);
  });
});
