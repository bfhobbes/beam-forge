/**
 * Parametric shapes: rectangles, ellipses and polygons keep their size and
 * radii until converted to a path. Their outlines live here so the canvas,
 * the drawing tools and "Convert to path" share one definition, and match
 * the planner's (`crates/bf-core/src/plan/outline.rs`).
 */
import type {
  EllipseObject,
  PathObject,
  Point,
  PolygonObject,
  RectObject,
  SceneObject,
  Segment,
  Subpath,
} from "./model.js";
import { applyToPoint, type Transform } from "./transform.js";

export type ShapeObject = RectObject | EllipseObject | PolygonObject;

/** The size and radius fields a shape can change, by shape. */
export const SHAPE_FIELDS = {
  rect: ["width", "height", "cornerRadius"],
  ellipse: ["rx", "ry"],
  polygon: ["sides", "radius", "innerRadius"],
} as const;

/** Size and radius edits; each key must belong to the shape it is applied to. */
export interface ShapeChanges {
  width?: number;
  height?: number;
  cornerRadius?: number;
  rx?: number;
  ry?: number;
  sides?: number;
  radius?: number;
  innerRadius?: number | null;
}

/** Most sides a polygon may have. */
export const MAX_SIDES = 100;

export function isShape(object: SceneObject): object is ShapeObject {
  return object.type === "rect" || object.type === "ellipse" || object.type === "polygon";
}

/**
 * Whether `object` is or holds a shape or text, which converting to a path
 * would change.
 */
export function hasShape(object: SceneObject): boolean {
  if (object.type === "group") return object.children.some(hasShape);
  return isShape(object) || object.type === "text";
}

/** Why `shape`'s size or radii can't be used, or null if they are fine. */
export function shapeProblem(shape: ShapeObject): string | null {
  const positive = (value: number) => Number.isFinite(value) && value > 0;
  switch (shape.type) {
    case "rect":
      if (!positive(shape.width) || !positive(shape.height)) return "Size must be above zero";
      if (!Number.isFinite(shape.cornerRadius) || shape.cornerRadius < 0) {
        return "Corner radius can't be negative";
      }
      return null;
    case "ellipse":
      return positive(shape.rx) && positive(shape.ry) ? null : "Size must be above zero";
    case "polygon":
      if (!Number.isInteger(shape.sides) || shape.sides < 3 || shape.sides > MAX_SIDES) {
        return `Sides is a whole number from 3 to ${MAX_SIDES}`;
      }
      if (!positive(shape.radius)) return "Radius must be above zero";
      if (shape.innerRadius !== null && !positive(shape.innerRadius)) {
        return "Inner radius must be above zero";
      }
      return null;
  }
}

/** Kappa: control-point distance for a quarter circle drawn as one cubic. */
const KAPPA = 0.5522847498;

const line = (x: number, y: number): Segment => ({ kind: "line", to: { x, y } });

/** Origin at the bottom-left corner; rounded corners are quarter-circle cubics. */
function rectOutline(width: number, height: number, cornerRadius: number): Subpath {
  const r = Math.max(0, Math.min(cornerRadius, width / 2, height / 2));
  if (r === 0) {
    return {
      start: { x: 0, y: 0 },
      segments: [line(width, 0), line(width, height), line(0, height)],
      closed: true,
    };
  }
  const k = r * KAPPA;
  const corner = (cx: number, cy: number, x: number, y: number, c2x: number, c2y: number) =>
    ({
      kind: "cubic",
      control1: { x: cx, y: cy },
      control2: { x: c2x, y: c2y },
      to: { x, y },
    }) as const;
  return {
    start: { x: r, y: 0 },
    segments: [
      line(width - r, 0),
      corner(width - r + k, 0, width, r, width, r - k),
      line(width, height - r),
      corner(width, height - r + k, width - r, height, width - r + k, height),
      line(r, height),
      corner(r - k, height, 0, height - r, 0, height - r + k),
      line(0, r),
      corner(0, r - k, r, 0, r - k, 0),
    ],
    closed: true,
  };
}

/** Centred on the origin, as four cubics. */
function ellipseOutline(rx: number, ry: number): Subpath {
  const kx = rx * KAPPA;
  const ky = ry * KAPPA;
  const quarter = (c1: Point, c2: Point, to: Point): Segment => ({
    kind: "cubic",
    control1: c1,
    control2: c2,
    to,
  });
  return {
    start: { x: rx, y: 0 },
    segments: [
      quarter({ x: rx, y: ky }, { x: kx, y: ry }, { x: 0, y: ry }),
      quarter({ x: -kx, y: ry }, { x: -rx, y: ky }, { x: -rx, y: 0 }),
      quarter({ x: -rx, y: -ky }, { x: -kx, y: -ry }, { x: 0, y: -ry }),
      quarter({ x: kx, y: -ry }, { x: rx, y: -ky }, { x: rx, y: 0 }),
    ],
    closed: true,
  };
}

/** First vertex points straight up; a star alternates outer and inner radii. */
function polygonOutline(sides: number, radius: number, innerRadius: number | null): Subpath {
  const count = innerRadius === null ? sides : sides * 2;
  const points = Array.from({ length: count }, (_, i) => {
    const r = innerRadius !== null && i % 2 === 1 ? innerRadius : radius;
    const angle = Math.PI / 2 + (i * 2 * Math.PI) / count;
    return { x: r * Math.cos(angle), y: r * Math.sin(angle) };
  });
  return {
    start: points[0],
    segments: points.slice(1).map((to) => ({ kind: "line", to })),
    closed: true,
  };
}

/** A leaf object's outline in its own (object) space. */
export function outline(object: Exclude<SceneObject, { type: "group" }>): Subpath[] {
  switch (object.type) {
    case "path":
      return object.subpaths;
    case "text":
      return object.outline;
    case "rect":
      return [rectOutline(object.width, object.height, object.cornerRadius)];
    case "ellipse":
      return [ellipseOutline(object.rx, object.ry)];
    case "polygon":
      return [polygonOutline(object.sides, object.radius, object.innerRadius)];
  }
}

/** Affine maps keep Béziers Béziers, so transforming the control points is exact. */
export function transformSubpath(transform: Transform, subpath: Subpath): Subpath {
  const at = (point: Point) => applyToPoint(transform, point);
  return {
    start: at(subpath.start),
    closed: subpath.closed,
    segments: subpath.segments.map((segment): Segment => {
      switch (segment.kind) {
        case "line":
          return { kind: "line", to: at(segment.to) };
        case "quad":
          return { kind: "quad", control: at(segment.control), to: at(segment.to) };
        case "cubic":
          return {
            kind: "cubic",
            control1: at(segment.control1),
            control2: at(segment.control2),
            to: at(segment.to),
          };
      }
    }),
  };
}

/**
 * `object` with every shape in it replaced by a path of the same outline,
 * id, name, layer and transform, so it draws and cuts exactly the same.
 */
export function toPath(object: SceneObject): SceneObject {
  if (object.type === "group") return { ...object, children: object.children.map(toPath) };
  if (object.type === "path") return object;
  const { id, name, layerId, transform } = object;
  const path: PathObject = {
    type: "path",
    id,
    name,
    layerId,
    transform,
    subpaths: outline(object),
  };
  return path;
}
