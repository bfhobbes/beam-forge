/**
 * Transform maths for the selection, shared by the canvas handles and the
 * Inspector's numeric fields so both produce identical edits. Everything
 * works on the selection's axis-aligned world bounds.
 */
import { setTransforms, type Command } from "#lib/document/commands.js";
import type { DesignDocument, Point } from "#lib/document/model.js";
import { multiply, rotate, scale, translate, type Transform } from "#lib/document/transform.js";
import { buildScene } from "./scene.js";
import type { Bounds } from "./view.js";

/** A point on a box as fractions of its size: `{x: 0, y: 0}` is bottom-left (Y up). */
export interface Anchor {
  x: 0 | 0.5 | 1;
  y: 0 | 0.5 | 1;
}

export const CENTRE: Anchor = { x: 0.5, y: 0.5 };

export type HandleId = "nw" | "n" | "ne" | "e" | "se" | "s" | "sw" | "w";

/** Where each scale handle sits on the selection box. */
export const HANDLE_ANCHORS: Readonly<Record<HandleId, Anchor>> = {
  nw: { x: 0, y: 1 },
  n: { x: 0.5, y: 1 },
  ne: { x: 1, y: 1 },
  e: { x: 1, y: 0.5 },
  se: { x: 1, y: 0 },
  s: { x: 0.5, y: 0 },
  sw: { x: 0, y: 0 },
  w: { x: 0, y: 0.5 },
};

export type Gesture = { kind: "move" } | { kind: "scale"; handle: HandleId } | { kind: "rotate" };

export interface Modifiers {
  /** Scale: keep the aspect ratio. Rotate: snap to 15° steps. */
  shift: boolean;
  /** Scale about the centre instead of the opposite handle. */
  alt: boolean;
}

const ROTATE_SNAP = Math.PI / 12;
/** Smallest scale factor a drag can reach, so objects never collapse to nothing. */
const MIN_SCALE = 1e-4;

export function anchorPoint(bounds: Bounds, anchor: Anchor): Point {
  return {
    x: bounds.minX + (bounds.maxX - bounds.minX) * anchor.x,
    y: bounds.minY + (bounds.maxY - bounds.minY) * anchor.y,
  };
}

/** `transform` applied about `point` instead of the origin. */
function about(point: Point, transform: Transform): Transform {
  return multiply(translate(point.x, point.y), multiply(transform, translate(-point.x, -point.y)));
}

/** World bounds of some top-level objects, or null for none. */
export function selectionBounds(doc: DesignDocument, ids: readonly string[]): Bounds | null {
  const wanted = new Set(ids);
  return buildScene({ ...doc, objects: doc.objects.filter((object) => wanted.has(object.id)) })
    .bounds;
}

/**
 * Sets each top-level object's transform to `by` applied on top of its
 * transform in `origin` (defaulting to its current one). Drags pass the
 * transforms from the start of the gesture so updates never accumulate error.
 */
export function transformObjects(
  doc: DesignDocument,
  ids: readonly string[],
  by: Transform,
  origin: Readonly<Record<string, Transform>> = {},
): Command {
  const wanted = new Set(ids);
  return setTransforms(
    Object.fromEntries(
      doc.objects
        .filter((object) => wanted.has(object.id))
        .map((object) => [object.id, multiply(by, origin[object.id] ?? object.transform)]),
    ),
  );
}

const sign = (value: number) => (value < 0 ? -1 : 1);
const clampScale = (value: number) => sign(value) * Math.max(MIN_SCALE, Math.abs(value));

/** The world transform for a drag from `from` to `to` (world mm) on a selection box. */
export function dragTransform(
  gesture: Gesture,
  bounds: Bounds,
  from: Point,
  to: Point,
  modifiers: Modifiers,
): Transform {
  switch (gesture.kind) {
    case "move":
      return translate(to.x - from.x, to.y - from.y);
    case "rotate": {
      const centre = anchorPoint(bounds, CENTRE);
      let angle =
        Math.atan2(to.y - centre.y, to.x - centre.x) -
        Math.atan2(from.y - centre.y, from.x - centre.x);
      if (modifiers.shift) angle = Math.round(angle / ROTATE_SNAP) * ROTATE_SNAP;
      return about(centre, rotate(angle));
    }
    case "scale": {
      const handle = HANDLE_ANCHORS[gesture.handle];
      const pivot = anchorPoint(
        bounds,
        modifiers.alt
          ? CENTRE
          : { x: (1 - handle.x) as Anchor["x"], y: (1 - handle.y) as Anchor["y"] },
      );
      const start = anchorPoint(bounds, handle);
      const ratio = (axis: "x" | "y") => {
        const span = start[axis] - pivot[axis];
        if (handle[axis] === 0.5 || span === 0) return null;
        return clampScale((start[axis] + to[axis] - from[axis] - pivot[axis]) / span);
      };
      let sx = ratio("x");
      let sy = ratio("y");
      if (modifiers.shift) {
        const uniform = Math.max(Math.abs(sx ?? 0), Math.abs(sy ?? 0)) || 1;
        sx = sign(sx ?? 1) * uniform;
        sy = sign(sy ?? 1) * uniform;
      }
      return about(pivot, scale(sx ?? 1, sy ?? 1));
    }
  }
}

/** The Inspector's numeric fields, in mm and degrees. */
export interface Properties {
  /** Position of the anchor point. */
  x: number;
  y: number;
  width: number;
  height: number;
  /** Anticlockwise; one object's own rotation, 0 for several. */
  rotation: number;
}

export function readProperties(
  doc: DesignDocument,
  ids: readonly string[],
  bounds: Bounds,
  anchor: Anchor,
): Properties {
  const { x, y } = anchorPoint(bounds, anchor);
  const single = ids.length === 1 ? doc.objects.find((object) => object.id === ids[0]) : undefined;
  const [a, b] = single?.transform ?? [1, 0];
  return {
    x,
    y,
    width: bounds.maxX - bounds.minX,
    height: bounds.maxY - bounds.minY,
    rotation: (Math.atan2(b, a) * 180) / Math.PI,
  };
}

/**
 * The world transform that sets one property to `value`, keeping the anchor
 * point fixed for size and rotation. Null when the value cannot apply: a
 * size of zero or less, or resizing a box that has no extent on that axis.
 */
export function propertyTransform(
  current: Properties,
  bounds: Bounds,
  anchor: Anchor,
  key: keyof Properties,
  value: number,
): Transform | null {
  const pivot = anchorPoint(bounds, anchor);
  switch (key) {
    case "x":
      return translate(value - current.x, 0);
    case "y":
      return translate(0, value - current.y);
    case "width":
      if (value <= 0 || current.width === 0) return null;
      return about(pivot, scale(value / current.width, 1));
    case "height":
      if (value <= 0 || current.height === 0) return null;
      return about(pivot, scale(1, value / current.height));
    case "rotation":
      return about(pivot, rotate(((value - current.rotation) * Math.PI) / 180));
  }
}

export type AlignEdge = "left" | "centre" | "right" | "top" | "middle" | "bottom";

/** Where each edge sits on a box, and on which axis. */
const ALIGN: Readonly<Record<AlignEdge, { axis: "x" | "y"; at: 0 | 0.5 | 1 }>> = {
  left: { axis: "x", at: 0 },
  centre: { axis: "x", at: 0.5 },
  right: { axis: "x", at: 1 },
  bottom: { axis: "y", at: 0 },
  middle: { axis: "y", at: 0.5 },
  top: { axis: "y", at: 1 },
};

function edgeOf(bounds: Bounds, axis: "x" | "y", at: number): number {
  const [min, max] = axis === "x" ? [bounds.minX, bounds.maxX] : [bounds.minY, bounds.maxY];
  return min + (max - min) * at;
}

/** Moves each object by its own translation; objects not listed stay put. */
function moveEach(doc: DesignDocument, moves: ReadonlyMap<string, Point>): Command {
  return setTransforms(
    Object.fromEntries(
      doc.objects
        .filter((object) => moves.has(object.id))
        .map((object) => {
          const { x, y } = moves.get(object.id)!;
          return [object.id, multiply(translate(x, y), object.transform)];
        }),
    ),
  );
}

/**
 * Lines up each top-level object's edge or centre with the same edge of
 * `target`: the selection's bounds, or the bed's.
 */
export function alignObjects(
  doc: DesignDocument,
  ids: readonly string[],
  edge: AlignEdge,
  target: Bounds,
): Command {
  const scene = buildScene(doc);
  const { axis, at } = ALIGN[edge];
  const goal = edgeOf(target, axis, at);
  const moves = new Map<string, Point>();
  for (const id of ids) {
    const bounds = scene.boundsOf([id]);
    if (!bounds) continue;
    const by = goal - edgeOf(bounds, axis, at);
    moves.set(id, axis === "x" ? { x: by, y: 0 } : { x: 0, y: by });
  }
  return moveEach(doc, moves);
}

/**
 * Spaces top-level objects evenly along an axis: the outermost two stay put
 * and the gaps between neighbouring boxes become equal.
 */
export function distributeObjects(
  doc: DesignDocument,
  ids: readonly string[],
  axis: "x" | "y",
): Command {
  const scene = buildScene(doc);
  const boxes = ids
    .flatMap((id) => {
      const bounds = scene.boundsOf([id]);
      return bounds ? [{ id, bounds }] : [];
    })
    .map(({ id, bounds }) => ({
      id,
      min: axis === "x" ? bounds.minX : bounds.minY,
      size: axis === "x" ? bounds.maxX - bounds.minX : bounds.maxY - bounds.minY,
    }))
    .toSorted((a, b) => a.min - b.min);
  const moves = new Map<string, Point>();
  if (boxes.length >= 3) {
    const first = boxes[0];
    const last = boxes[boxes.length - 1];
    const sizes = boxes.reduce((sum, box) => sum + box.size, 0);
    const gap = (last.min + last.size - first.min - sizes) / (boxes.length - 1);
    let at = first.min;
    for (const box of boxes) {
      const by = at - box.min;
      if (Math.abs(by) > 1e-9) moves.set(box.id, axis === "x" ? { x: by, y: 0 } : { x: 0, y: by });
      at += box.size + gap;
    }
  }
  return moveEach(doc, moves);
}

/** Flips top-level objects about the centre of their combined bounds: "horizontal" swaps left and right. */
export function mirrorObjects(
  doc: DesignDocument,
  ids: readonly string[],
  direction: "horizontal" | "vertical",
): Command | null {
  const bounds = selectionBounds(doc, ids);
  if (!bounds) return null;
  const flip = direction === "horizontal" ? scale(-1, 1) : scale(1, -1);
  return transformObjects(doc, ids, about(anchorPoint(bounds, CENTRE), flip));
}
