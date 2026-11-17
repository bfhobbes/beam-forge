import { describe, expect, it } from "vite-plus/test";
import { dragBox, pathFromPoints, shapeFromDrag, type ShapeDrag } from "./draw.js";

const base = { id: "new", layerId: "layer-1" };
const drag = (fx: number, fy: number, tx: number, ty: number, shift = false, alt = false) =>
  ({ from: { x: fx, y: fy }, to: { x: tx, y: ty }, shift, alt }) satisfies ShapeDrag;

describe("drag box", () => {
  it("spans the drag in any direction", () => {
    expect(dragBox(drag(10, 20, 4, 30))).toEqual({ minX: 4, minY: 20, maxX: 10, maxY: 30 });
  });

  it("is square with shift, the longer side winning, towards the pointer", () => {
    expect(dragBox(drag(10, 10, 4, 30, true))).toEqual({ minX: -10, minY: 10, maxX: 10, maxY: 30 });
  });

  it("grows from the centre with alt", () => {
    expect(dragBox(drag(10, 10, 13, 5, false, true))).toEqual({
      minX: 7,
      minY: 5,
      maxX: 13,
      maxY: 15,
    });
  });
});

describe("shapes from drags", () => {
  it("makes a rectangle filling the box from its bottom-left corner", () => {
    expect(shapeFromDrag("rectangle", drag(30, 40, 10, 15), base)).toEqual({
      type: "rect",
      id: "new",
      name: "Rectangle",
      layerId: "layer-1",
      transform: [1, 0, 0, 1, 10, 15],
      width: 20,
      height: 25,
      cornerRadius: 0,
    });
  });

  it("makes an ellipse centred in the box, a circle with shift", () => {
    expect(shapeFromDrag("ellipse", drag(0, 0, 20, 10), base)).toMatchObject({
      type: "ellipse",
      transform: [1, 0, 0, 1, 10, 5],
      rx: 10,
      ry: 5,
    });
    expect(shapeFromDrag("ellipse", drag(0, 0, 20, 10, true), base)).toMatchObject({
      rx: 10,
      ry: 10,
    });
  });

  it("makes a polygon centred where the drag starts, reaching the pointer", () => {
    expect(shapeFromDrag("polygon", drag(50, 50, 53, 54), base)).toMatchObject({
      type: "polygon",
      transform: [1, 0, 0, 1, 50, 50],
      sides: 6,
      radius: 5,
      innerRadius: null,
    });
  });

  it("makes nothing from a click or a flat drag", () => {
    expect(shapeFromDrag("rectangle", drag(5, 5, 5, 5), base)).toBeNull();
    expect(shapeFromDrag("rectangle", drag(5, 5, 30, 5), base)).toBeNull();
    expect(shapeFromDrag("ellipse", drag(5, 5, 5, 30), base)).toBeNull();
    expect(shapeFromDrag("polygon", drag(5, 5, 5, 5), base)).toBeNull();
  });
});

describe("paths from points", () => {
  const points = [
    { x: 0, y: 0 },
    { x: 10, y: 0 },
    { x: 10, y: 10 },
  ];

  it("joins the points, open or closed", () => {
    expect(pathFromPoints(points, false, base)).toMatchObject({
      type: "path",
      name: "Polyline",
      subpaths: [
        {
          start: { x: 0, y: 0 },
          segments: [
            { kind: "line", to: { x: 10, y: 0 } },
            { kind: "line", to: { x: 10, y: 10 } },
          ],
          closed: false,
        },
      ],
    });
    expect(pathFromPoints(points, true, base)?.subpaths[0].closed).toBe(true);
    expect(pathFromPoints(points.slice(0, 2), false, base)?.name).toBe("Line");
  });

  it("needs two points for a line and three to close", () => {
    expect(pathFromPoints(points.slice(0, 1), false, base)).toBeNull();
    expect(pathFromPoints(points.slice(0, 2), true, base)).toBeNull();
  });
});
