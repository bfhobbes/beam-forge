/**
 * Marquee (rubber-band) selection. Like CAD tools, the drag direction picks
 * the rule: left to right is a window that selects objects entirely inside;
 * right to left is a crossing box that selects anything it touches.
 */
import type { Point } from "#lib/document/model.js";
import type { Scene, SceneItem } from "./scene.js";
import type { Bounds } from "./view.js";

export type MarqueeMode = "window" | "crossing";

/** Decided from screen positions of where the drag started and where it is now. */
export function marqueeMode(from: Point, to: Point): MarqueeMode {
  return to.x >= from.x ? "window" : "crossing";
}

const contains = (box: Bounds, inner: Bounds) =>
  inner.minX >= box.minX &&
  inner.maxX <= box.maxX &&
  inner.minY >= box.minY &&
  inner.maxY <= box.maxY;

/** Liang–Barsky: whether the segment a→b passes through the box. */
function segmentHitsBox(a: Point, b: Point, box: Bounds): boolean {
  const dx = b.x - a.x;
  const dy = b.y - a.y;
  let enter = 0;
  let leave = 1;
  const edges: [number, number][] = [
    [-dx, a.x - box.minX],
    [dx, box.maxX - a.x],
    [-dy, a.y - box.minY],
    [dy, box.maxY - a.y],
  ];
  for (const [p, q] of edges) {
    if (p === 0) {
      if (q < 0) return false;
    } else {
      const t = q / p;
      if (p < 0) enter = Math.max(enter, t);
      else leave = Math.min(leave, t);
      if (enter > leave) return false;
    }
  }
  return true;
}

function touches(item: SceneItem, box: Bounds): boolean {
  if (contains(box, item.bounds)) return true;
  return item.polylines.some((line) =>
    line.some((point, i) => i > 0 && segmentHitsBox(line[i - 1], point, box)),
  );
}

/** Top-level ids the marquee selects, in paint order. */
export function marqueeSelect(scene: Scene, box: Bounds, mode: MarqueeMode): string[] {
  const candidates = new Set(scene.query(box).map((item) => item.rootId));
  if (candidates.size === 0) return [];
  const byRoot = new Map<string, SceneItem[]>();
  for (const item of scene.items) {
    if (!candidates.has(item.rootId)) continue;
    byRoot.set(item.rootId, [...(byRoot.get(item.rootId) ?? []), item]);
  }
  return [...byRoot]
    .filter(([, items]) =>
      mode === "window"
        ? items.every((item) => contains(box, item.bounds))
        : items.some((item) => touches(item, box)),
    )
    .map(([rootId]) => rootId);
}
