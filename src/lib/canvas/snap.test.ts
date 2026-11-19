import { describe, expect, it } from "vite-plus/test";
import { insert } from "#lib/document/commands.js";
import { DocumentEditor } from "#lib/document/editor.svelte.js";
import { Lifecycle } from "#lib/desktop/lifecycle.svelte.js";
import type { SceneObject } from "#lib/document/model.js";
import { translate } from "#lib/document/transform.js";
import { buildScene } from "./scene.js";
import {
  boxGuides,
  dragTargets,
  pointGuides,
  SNAP_PX,
  snapAxis,
  snapBox,
  snapPoint,
  snapSettings,
  snapTargets,
  type SnapSettings,
} from "./snap.js";

const box = (minX: number, minY: number, maxX: number, maxY: number) => ({
  minX,
  minY,
  maxX,
  maxY,
});

const noGrid: SnapSettings = { reach: 1, grid: null };

describe("snap targets", () => {
  it("lists each box's edges and centre per axis, sorted, plus the bed", () => {
    const targets = snapTargets([box(50, 0, 60, 10), box(0, 20, 10, 40)], box(0, 0, 400, 300));
    expect(targets.x.map((line) => line.at)).toEqual([0, 0, 5, 10, 50, 55, 60, 200, 400]);
    expect(targets.y.map((line) => line.at)).toEqual([0, 0, 5, 10, 20, 30, 40, 150, 300]);
    // A line spans its box on the other axis, the bed's spans the bed.
    expect(targets.x.find((line) => line.at === 55)).toEqual({
      at: 55,
      from: 0,
      to: 10,
    });
    expect(targets.x.find((line) => line.at === 200)).toEqual({
      at: 200,
      from: 0,
      to: 300,
    });
  });
});

describe("snapAxis", () => {
  const lines = snapTargets([box(10, 0, 20, 10)], null).x; // 10, 15, 20

  it("shifts the anchor nearest a line onto it", () => {
    expect(snapAxis(lines, [9.5], [], noGrid)).toBeCloseTo(0.5);
    expect(snapAxis(lines, [3, 20.25, 40], [], noGrid)).toBeCloseTo(-0.25);
  });

  it("leaves things out of reach alone", () => {
    expect(snapAxis(lines, [12], [12], noGrid)).toBe(0);
  });

  it("falls back to the grid only when no line is in reach", () => {
    const settings = { reach: 1, grid: 2.5 };
    expect(snapAxis(lines, [12.2], [12.2], settings)).toBeCloseTo(0.3);
    // A line within reach beats a nearer grid line.
    expect(snapAxis(lines, [9.2], [9.2], { reach: 1, grid: 0.5 })).toBeCloseTo(0.8);
  });
});

describe("snapBox", () => {
  const targets = snapTargets([box(10, 10, 20, 20)], box(0, 0, 100, 100));

  it("snaps a moved box's edges or centre to other boxes", () => {
    // Left edge 20.4 to the other box's right edge at 20; bottom 9.7 to its bottom at 10.
    expect(snapBox(targets, box(20.4, 9.7, 25.4, 14.7), noGrid)).toEqual({
      x: expect.closeTo(-0.4),
      y: expect.closeTo(0.3),
    });
    // Centre 49.6 to the bed's centre at 50.
    expect(snapBox(targets, box(39.6, 70, 59.6, 80), noGrid).x).toBeCloseTo(0.4);
  });

  it("snaps edges, not the centre, to the grid", () => {
    const settings = { reach: 0.4, grid: 1 };
    // Edges 30.3 and 33.3 sit 0.3 off the grid; the centre 31.8 is 0.2 off a line, but does not count.
    expect(snapBox(targets, box(30.3, 30, 33.3, 31), settings).x).toBeCloseTo(-0.3);
  });
});

describe("snapPoint", () => {
  const targets = snapTargets([box(10, 10, 20, 20)], null);

  it("snaps both coordinates", () => {
    const point = snapPoint(targets, { x: 19.5, y: 15.5 }, noGrid);
    expect(point.x).toBeCloseTo(20);
    expect(point.y).toBeCloseTo(15);
  });

  it("keeps an axis it may not move", () => {
    expect(snapPoint(targets, { x: 19.5, y: 15.5 }, noGrid, { x: true, y: false }).y).toBe(15.5);
  });
});

describe("guides", () => {
  const targets = snapTargets([box(10, 10, 20, 20), box(10, 50, 30, 60)], null);

  it("draws one guide per aligned position, spanning everything on it", () => {
    const guides = boxGuides(targets, box(10, 30, 15, 40));
    expect(guides).toEqual([
      // Left edges line up with both boxes: one guide from the lowest to the highest.
      { axis: "x", at: 10, from: 10, to: 60 },
      // Its right edge meets the first box's centre.
      { axis: "x", at: 15, from: 10, to: 40 },
    ]);
  });

  it("draws nothing when nothing lines up", () => {
    expect(boxGuides(targets, box(11, 31, 12, 32))).toEqual([]);
  });

  it("guides a point", () => {
    // x = 20 is the first box's right edge and the second's centre.
    expect(pointGuides(targets, { x: 20, y: 70 })).toEqual([
      { axis: "x", at: 20, from: 10, to: 70 },
    ]);
  });
});

describe("snapSettings", () => {
  it("measures reach in screen pixels and follows the drawn grid", () => {
    const options = { enabled: true, grid: true, bed: null, area: null };
    expect(snapSettings(2, options).reach).toBe(SNAP_PX / 2);
    expect(snapSettings(10, options).grid).toBe(1);
    expect(snapSettings(10, { ...options, grid: false }).grid).toBeNull();
  });
});

describe("dragTargets", () => {
  const rect = (id: string, x: number): SceneObject => ({
    type: "rect",
    id,
    name: id,
    layerId: "layer-1",
    transform: translate(x, 0),
    width: 10,
    height: 10,
    cornerRadius: 0,
  });

  it("uses objects in view, less the ones being dragged, and the bed", () => {
    const editor = new DocumentEditor(new Lifecycle());
    editor.execute(insert([rect("a", 0), rect("b", 100), rect("c", 1000)]));
    const scene = buildScene(editor.doc);
    const targets = dragTargets(
      scene,
      {
        enabled: true,
        grid: false,
        bed: box(0, 0, 400, 300),
        area: box(-10, -10, 500, 500),
      },
      ["a"],
    );
    expect(targets.x.map((line) => line.at)).toEqual([0, 100, 105, 110, 200, 400]);
  });
});
