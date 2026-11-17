/**
 * Shapes from drawing gestures, in world millimetres: a drag makes a
 * rectangle, ellipse or polygon; clicked points make a line or polyline.
 * Pure, so the tool's maths is testable without a canvas.
 */
import type { PathObject, Point } from "#lib/document/model.js";
import type { ShapeObject } from "#lib/document/shapes.js";
import { IDENTITY, translate } from "#lib/document/transform.js";
import type { Modifiers } from "./arrange.js";
import type { Bounds } from "./view.js";

export type ShapeTool = "rectangle" | "ellipse" | "line" | "polygon";

/** Sides a new polygon gets; the Properties tab changes them afterwards. */
export const DEFAULT_SIDES = 6;

/** Smallest size a drag makes a shape of, mm: anything smaller was a click. */
export const MIN_SIZE = 0.01;

export interface ShapeDrag extends Modifiers {
  from: Point;
  to: Point;
}

/** What a new object is called and which layer it goes on. */
export interface NewObject {
  id: string;
  layerId: string;
}

/**
 * The box a drag spans: corner to corner, or with alt from the centre out.
 * Shift makes it square, the longer side winning.
 */
export function dragBox({ from, to, shift, alt }: ShapeDrag): Bounds {
  let dx = to.x - from.x;
  let dy = to.y - from.y;
  if (shift) {
    const side = Math.max(Math.abs(dx), Math.abs(dy));
    dx = (dx < 0 ? -1 : 1) * side;
    dy = (dy < 0 ? -1 : 1) * side;
  }
  if (alt) {
    const [w, h] = [Math.abs(dx), Math.abs(dy)];
    return { minX: from.x - w, minY: from.y - h, maxX: from.x + w, maxY: from.y + h };
  }
  return {
    minX: Math.min(from.x, from.x + dx),
    minY: Math.min(from.y, from.y + dy),
    maxX: Math.max(from.x, from.x + dx),
    maxY: Math.max(from.y, from.y + dy),
  };
}

/**
 * The shape a drag with `tool` makes, or null when the drag is too small.
 * Rectangles and ellipses fill the drag box; a polygon is centred where the
 * drag starts, with a vertex radius reaching the pointer.
 */
export function shapeFromDrag(
  tool: Exclude<ShapeTool, "line">,
  drag: ShapeDrag,
  { id, layerId }: NewObject,
  sides = DEFAULT_SIDES,
): ShapeObject | null {
  if (tool === "polygon") {
    const radius = Math.hypot(drag.to.x - drag.from.x, drag.to.y - drag.from.y);
    if (radius < MIN_SIZE) return null;
    return {
      type: "polygon",
      id,
      name: "Polygon",
      layerId,
      transform: translate(drag.from.x, drag.from.y),
      sides,
      radius,
      innerRadius: null,
    };
  }
  const box = dragBox(drag);
  const width = box.maxX - box.minX;
  const height = box.maxY - box.minY;
  if (width < MIN_SIZE || height < MIN_SIZE) return null;
  if (tool === "rectangle") {
    return {
      type: "rect",
      id,
      name: "Rectangle",
      layerId,
      transform: translate(box.minX, box.minY),
      width,
      height,
      cornerRadius: 0,
    };
  }
  return {
    type: "ellipse",
    id,
    name: "Ellipse",
    layerId,
    transform: translate((box.minX + box.maxX) / 2, (box.minY + box.maxY) / 2),
    rx: width / 2,
    ry: height / 2,
  };
}

/**
 * A line through the clicked points, closed back to the first if `closed`,
 * or null when there aren't enough points for one.
 */
export function pathFromPoints(
  points: readonly Point[],
  closed: boolean,
  { id, layerId }: NewObject,
): PathObject | null {
  if (points.length < (closed ? 3 : 2)) return null;
  return {
    type: "path",
    id,
    name: points.length === 2 ? "Line" : "Polyline",
    layerId,
    transform: IDENTITY,
    subpaths: [
      {
        start: points[0],
        segments: points.slice(1).map((to) => ({ kind: "line", to })),
        closed,
      },
    ],
  };
}
