/**
 * Selection handles in screen space: eight scale handles on the selection
 * box and a rotate handle outside each corner. Drawing and hit-testing both
 * use these positions, so what you see is what you grab.
 */
import type { Point } from "#lib/document/model.js";
import { anchorPoint, HANDLE_ANCHORS, type HandleId } from "./arrange.js";
import { worldToScreen, type Bounds, type View } from "./view.js";

export type Corner = "nw" | "ne" | "se" | "sw";

export type Handle =
  | { kind: "scale"; id: HandleId; at: Point }
  | { kind: "rotate"; id: Corner; at: Point };

/** Scale handle square side, in CSS px. */
export const HANDLE_SIZE = 8;
/** Rotate handle radius, in CSS px. */
export const ROTATE_RADIUS = 4;
/** How far rotate handles sit out from their corner, in CSS px. */
const ROTATE_OFFSET = 18;
/** How close the pointer must be to grab a handle, in CSS px. */
const REACH = 8;
/** A box side shorter than this on screen shows only corner handles. */
const MIN_EDGE_PX = 24;

const corners: Corner[] = ["nw", "ne", "se", "sw"];
const edges: HandleId[] = ["n", "e", "s", "w"];

export function selectionHandles(view: View, bounds: Bounds): Handle[] {
  const at = (id: HandleId) => worldToScreen(view, anchorPoint(bounds, HANDLE_ANCHORS[id]));
  const width = (bounds.maxX - bounds.minX) * view.zoom;
  const height = (bounds.maxY - bounds.minY) * view.zoom;
  const handles: Handle[] = corners.map((id) => ({ kind: "scale", id, at: at(id) }));
  if (width >= MIN_EDGE_PX && height >= MIN_EDGE_PX) {
    for (const id of edges) handles.push({ kind: "scale", id, at: at(id) });
  }
  const diagonal = ROTATE_OFFSET / Math.SQRT2;
  for (const id of corners) {
    const corner = at(id);
    // Screen Y grows downwards, so north is -y.
    const dx = id.endsWith("e") ? diagonal : -diagonal;
    const dy = id.startsWith("n") ? -diagonal : diagonal;
    handles.push({ kind: "rotate", id, at: { x: corner.x + dx, y: corner.y + dy } });
  }
  return handles;
}

/** The handle nearest a screen point, if any is within reach. */
export function handleAt(handles: readonly Handle[], point: Point): Handle | null {
  let best: Handle | null = null;
  let bestDistance = REACH;
  for (const handle of handles) {
    const distance = Math.hypot(handle.at.x - point.x, handle.at.y - point.y);
    if (distance <= bestDistance) {
      best = handle;
      bestDistance = distance;
    }
  }
  return best;
}

/** The CSS cursor for hovering or dragging a handle. */
export function handleCursor(handle: Handle): string {
  if (handle.kind === "rotate") return "grab";
  switch (handle.id) {
    case "nw":
    case "se":
      return "nwse-resize";
    case "ne":
    case "sw":
      return "nesw-resize";
    case "n":
    case "s":
      return "ns-resize";
    case "e":
    case "w":
      return "ew-resize";
  }
}
