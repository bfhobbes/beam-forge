/**
 * Snapping maths for dragging on the canvas, Figma-style: a moving box (or
 * point) is pulled onto the edges and centres of other objects and of the
 * bed, or to an equal spacing with its neighbours, falling back to the grid.
 * Alignment targets draw guide lines and spacing draws gap hints; the grid
 * draws nothing. Pure, so it tests without a canvas.
 *
 * Targets are built once per drag from the objects in view: lines kept
 * sorted and boxes in an R-tree, so each pointer move is a binary search and
 * a query of the moving box's row rather than a scan of the scene.
 */
import RBush from "rbush";
import type { Point } from "#lib/document/model.js";
import { unionBounds, type Scene } from "./scene.js";
import { GRID_MIN_PX, gridSteps, type Bounds } from "./view.js";

/** How close something must come to snap, in CSS px. */
export const SNAP_PX = 6;

/** Coordinates closer than this are the same line, in mm. */
const SAME = 1e-6;

/**
 * One line something can snap to: `at` on its axis (an x for vertical
 * lines), spanning `from`–`to` on the other axis, for drawing the guide.
 */
export interface SnapLine {
  at: number;
  from: number;
  to: number;
}

/** Snap lines per axis, each sorted by `at`, and the objects' boxes for spacing. */
export interface SnapTargets {
  x: readonly SnapLine[];
  y: readonly SnapLine[];
  /** Other objects' boxes (not the bed's). */
  boxes: RBush<Bounds>;
  /** All of `boxes`, or null for none. */
  extent: Bounds | null;
}

/** How a drag snaps right now. */
export interface SnapSettings {
  /** The snap distance in mm: `SNAP_PX` at the current zoom. */
  reach: number;
  /** Grid spacing in mm to fall back to, or null to skip the grid. */
  grid: number | null;
}

/** A guide line to draw: vertical at x = `at` for axis "x", horizontal for "y"; world mm. */
export interface Guide {
  axis: "x" | "y";
  at: number;
  from: number;
  to: number;
}

/**
 * An equal-spacing hint: a gap along `axis` from `from` to `to`, drawn at
 * `at` on the other axis; world mm.
 */
export interface Gap {
  axis: "x" | "y";
  from: number;
  to: number;
  at: number;
}

/** Lines along one axis of a box: its two edges and its centre. */
function boxLines(box: Bounds, axis: "x" | "y"): SnapLine[] {
  const [min, max, from, to] =
    axis === "x"
      ? [box.minX, box.maxX, box.minY, box.maxY]
      : [box.minY, box.maxY, box.minX, box.maxX];
  return [min, (min + max) / 2, max].map((at) => ({ at, from, to }));
}

/** Snap targets from other objects' boxes and the bed, edges and centres of each. */
export function snapTargets(boxes: Iterable<Bounds>, bed: Bounds | null): SnapTargets {
  const x: SnapLine[] = [];
  const y: SnapLine[] = [];
  const add = (box: Bounds) => {
    x.push(...boxLines(box, "x"));
    y.push(...boxLines(box, "y"));
  };
  const list = [...boxes];
  for (const box of list) add(box);
  if (bed) add(bed);
  const byAt = (a: SnapLine, b: SnapLine) => a.at - b.at;
  return {
    x: x.sort(byAt),
    y: y.sort(byAt),
    boxes: new RBush<Bounds>().load(list),
    extent: unionBounds(list),
  };
}

/** Index of the first line at or after `value`. */
function lowerBound(lines: readonly SnapLine[], value: number): number {
  let low = 0;
  let high = lines.length;
  while (low < high) {
    const mid = (low + high) >>> 1;
    if (lines[mid].at < value) low = mid + 1;
    else high = mid;
  }
  return low;
}

/** The nearest line to `value` within `reach`, or null. */
function nearest(lines: readonly SnapLine[], value: number, reach: number): SnapLine | null {
  const i = lowerBound(lines, value);
  let best: SnapLine | null = null;
  for (const line of [lines[i - 1], lines[i]]) {
    if (line && Math.abs(line.at - value) <= reach) {
      if (!best || Math.abs(line.at - value) < Math.abs(best.at - value)) best = line;
    }
  }
  return best;
}

/**
 * The shift along one axis that lands one of `anchors` on a target line (or
 * is one of the `extra` shifts, such as to an equal spacing), or failing
 * that one of `gridAnchors` on the grid; 0 when nothing is in reach.
 */
export function snapAxis(
  lines: readonly SnapLine[],
  anchors: readonly number[],
  gridAnchors: readonly number[],
  settings: SnapSettings,
  extra: readonly number[] = [],
): number {
  let best: number | null = null;
  const consider = (shift: number) => {
    if (Math.abs(shift) <= settings.reach && (best === null || Math.abs(shift) < Math.abs(best)))
      best = shift;
  };
  for (const shift of extra) consider(shift);
  for (const anchor of anchors) {
    const line = nearest(lines, anchor, settings.reach);
    if (line) consider(line.at - anchor);
  }
  if (best !== null) return best;
  const { grid } = settings;
  if (grid !== null && grid > 0) {
    for (const anchor of gridAnchors) consider(Math.round(anchor / grid) * grid - anchor);
  }
  return best ?? 0;
}

const span = (box: Bounds, axis: "x" | "y"): [number, number] =>
  axis === "x" ? [box.minX, box.maxX] : [box.minY, box.maxY];

const other = (axis: "x" | "y") => (axis === "x" ? "y" : "x");

type Spacing = { start: number; pairs: [Bounds | null, Bounds | null][] };

/**
 * Where a box of this size could start along `axis` to space itself evenly
 * from its neighbours in the same row (column for "y"): midway between the
 * nearest one each side, or one gap on from a pair of neighbours on one
 * side. Each comes with the pairs of boxes whose gaps match, `null`
 * standing for the box itself.
 */
function spacings(targets: SnapTargets, box: Bounds, axis: "x" | "y"): Spacing[] {
  const { extent } = targets;
  if (!extent) return [];
  const [min, max] = span(box, axis);
  const [crossMin, crossMax] = span(box, other(axis));
  const row = targets.boxes.search(
    axis === "x"
      ? { minX: extent.minX, maxX: extent.maxX, minY: crossMin, maxY: crossMax }
      : { minX: crossMin, maxX: crossMax, minY: extent.minY, maxY: extent.maxY },
  );
  const lo = (b: Bounds) => span(b, axis)[0];
  const hi = (b: Bounds) => span(b, axis)[1];
  // Neighbours wholly before or after the box, nearest first.
  const before = row.filter((b) => hi(b) <= min).sort((a, b) => hi(b) - hi(a));
  const after = row.filter((b) => lo(b) >= max).sort((a, b) => lo(a) - lo(b));
  const size = max - min;
  const options: Spacing[] = [];
  const left = before.at(0);
  const right = after.at(0);
  if (left && right) {
    options.push({
      start: (hi(left) + lo(right) - size) / 2,
      pairs: [
        [left, null],
        [null, right],
      ],
    });
  }
  // The next neighbour out must clear the nearest one along the axis.
  const outer = left && before.find((b) => hi(b) <= lo(left));
  if (left && outer) {
    options.push({
      start: hi(left) + lo(left) - hi(outer),
      pairs: [
        [outer, left],
        [left, null],
      ],
    });
  }
  const beyond = right && after.find((b) => lo(b) >= hi(right));
  if (right && beyond) {
    options.push({
      start: lo(right) - (lo(beyond) - hi(right)) - size,
      pairs: [
        [null, right],
        [right, beyond],
      ],
    });
  }
  return options;
}

/**
 * The shift that snaps a box being moved: any edge or its centre onto a
 * target, or to an equal spacing with its neighbours, else an edge onto the
 * grid.
 */
export function snapBox(targets: SnapTargets, box: Bounds, settings: SnapSettings): Point {
  const shift = (axis: "x" | "y") => {
    const [min, max] = span(box, axis);
    const even = spacings(targets, box, axis).map((option) => option.start - min);
    return snapAxis(targets[axis], [min, (min + max) / 2, max], [min, max], settings, even);
  };
  return { x: shift("x"), y: shift("y") };
}

/** Gap hints for a box that sits at an equal spacing with its neighbours. */
export function spacingGaps(targets: SnapTargets, box: Bounds): Gap[] {
  const gaps: Gap[] = [];
  for (const axis of ["x", "y"] as const) {
    const [min] = span(box, axis);
    const option = spacings(targets, box, axis).find(
      (candidate) => Math.abs(candidate.start - min) <= SAME * Math.max(1, Math.abs(min)),
    );
    if (!option) continue;
    for (const [a, b] of option.pairs) {
      const first = a ?? box;
      const second = b ?? box;
      // Drawn across the middle of where the two boxes overlap.
      const [aMin, aMax] = span(first, other(axis));
      const [bMin, bMax] = span(second, other(axis));
      const at = (Math.max(aMin, bMin) + Math.min(aMax, bMax)) / 2;
      gaps.push({ axis, from: span(first, axis)[1], to: span(second, axis)[0], at });
    }
  }
  return gaps;
}

/**
 * Snaps a point (a drawing tool's corner, a resize handle) onto target
 * lines or the grid. `axes` limits which coordinates may move: a side
 * handle only drags one.
 */
export function snapPoint(
  targets: SnapTargets,
  point: Point,
  settings: SnapSettings,
  axes: { x: boolean; y: boolean } = { x: true, y: true },
): Point {
  const shift = (axis: "x" | "y") =>
    axes[axis] ? snapAxis(targets[axis], [point[axis]], [point[axis]], settings) : 0;
  return { x: point.x + shift("x"), y: point.y + shift("y") };
}

/**
 * Guides for every target line `values` sit exactly on, one per position,
 * spanning the lines there and `across` (the dragged thing's extent on the
 * other axis).
 */
function axisGuides(
  lines: readonly SnapLine[],
  axis: "x" | "y",
  values: readonly number[],
  across: [number, number],
): Guide[] {
  const guides: Guide[] = [];
  for (const value of values) {
    if (guides.some((guide) => Math.abs(guide.at - value) <= SAME)) continue;
    let [from, to] = across;
    let hit = false;
    for (let i = lowerBound(lines, value - SAME); i < lines.length; i++) {
      const line = lines[i];
      if (line.at > value + SAME) break;
      hit = true;
      from = Math.min(from, line.from);
      to = Math.max(to, line.to);
    }
    if (hit) guides.push({ axis, at: value, from, to });
  }
  return guides;
}

/** Guide lines for a box's edges and centres that sit on target lines. */
export function boxGuides(targets: SnapTargets, box: Bounds): Guide[] {
  const values = (axis: "x" | "y") => {
    const [min, max] = span(box, axis);
    return [min, (min + max) / 2, max];
  };
  return [
    ...axisGuides(targets.x, "x", values("x"), span(box, "y")),
    ...axisGuides(targets.y, "y", values("y"), span(box, "x")),
  ];
}

/** Guide lines for a point that sits on target lines. */
export function pointGuides(targets: SnapTargets, point: Point): Guide[] {
  return [
    ...axisGuides(targets.x, "x", [point.x], [point.y, point.y]),
    ...axisGuides(targets.y, "y", [point.y], [point.x, point.x]),
  ];
}

/** What the canvas tells a tool about snapping when a drag starts. */
export interface SnapOptions {
  /** Snapping is on in the view preferences. */
  enabled: boolean;
  /** Fall back to the grid. */
  grid: boolean;
  /** The bed, whose edges and centre are targets. */
  bed: Bounds | null;
  /** The world area in view: only objects there are targets. Null for all. */
  area: Bounds | null;
}

export const SNAP_OFF: SnapOptions = {
  enabled: false,
  grid: false,
  bed: null,
  area: null,
};

/** Snap settings at a zoom: the reach in mm, and the minor grid step the canvas draws. */
export function snapSettings(zoom: number, options: SnapOptions): SnapSettings {
  return {
    reach: SNAP_PX / zoom,
    grid: options.grid ? gridSteps(zoom, GRID_MIN_PX).minor : null,
  };
}

/**
 * Targets for a drag: the top-level objects in view, less the ones being
 * dragged, plus the bed. Uses the scene's R-tree, so it costs what is in
 * view rather than the whole document.
 */
export function dragTargets(
  scene: Scene,
  options: SnapOptions,
  exclude: Iterable<string> = [],
): SnapTargets {
  const skip = new Set(exclude);
  const roots = new Set<string>();
  for (const item of options.area ? scene.query(options.area) : scene.items) {
    if (!skip.has(item.rootId)) roots.add(item.rootId);
  }
  const boxes: Bounds[] = [];
  for (const id of roots) {
    const box = scene.rootBounds(id);
    if (box) boxes.push(box);
  }
  return snapTargets(boxes, options.bed);
}
