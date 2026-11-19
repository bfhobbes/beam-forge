/**
 * The select tool's pointer gestures on the canvas: click and shift-click
 * select, marquee select, and dragging the selection or its handles. Each
 * drag is one undo step: every update executes with the gesture's coalesce
 * key, rebuilt from the document as it was when the drag began. Moves and
 * resizes snap to other objects, the bed and the grid unless snapping is off
 * or ctrl is held; `guides` shows what they snapped to. Moves also snap to
 * equal spacing with neighbouring objects, shown by `gaps`.
 */
import { DocumentError } from "#lib/document/commands.js";
import type { DocumentEditor } from "#lib/document/editor.svelte.js";
import type { DesignDocument, Point } from "#lib/document/model.js";
import type { Selection } from "#lib/document/selection.svelte.js";
import { applyToPoint, type Transform } from "#lib/document/transform.js";
import {
  anchorPoint,
  dragTransform,
  HANDLE_ANCHORS,
  transformObjects,
  type Gesture,
  type Modifiers,
} from "./arrange.js";
import { handleAt, handleCursor, selectionHandles, type Handle } from "./handles.js";
import { marqueeMode, marqueeSelect, type MarqueeMode } from "./marquee.js";
import { unionBounds, type Scene } from "./scene.js";
import {
  boxGuides,
  dragTargets,
  SNAP_OFF,
  snapBox,
  snapPoint,
  snapSettings,
  spacingGaps,
  type Gap,
  type Guide,
  type SnapOptions,
  type SnapTargets,
} from "./snap.js";
import { screenToWorld, type Bounds, type View } from "./view.js";

/** A pointer event's position in CSS px from the canvas corner, and its modifiers. */
export interface PointerInput extends Modifiers {
  at: Point;
}

/** How close a click must be to an outline to pick it, in CSS px. */
export const PICK_REACH = 5;
/** How far the pointer must travel before a press becomes a drag, in CSS px. */
const DRAG_THRESHOLD = 3;

interface Press {
  start: Point;
  /** `start` in world mm, fixed at the press so panning mid-drag does not jump. */
  startWorld: Point;
  /** What a drag does: transform the selection, draw a marquee, or nothing. */
  gesture: Gesture | "marquee" | null;
  /** The object under the press, if any. */
  hit: string | null;
  additive: boolean;
  dragging: boolean;
  last: PointerInput;
  doc: DesignDocument;
  ids: string[];
  bounds: Bounds | null;
  coalesce: string;
  /** What a move or resize snaps to, gathered once the press becomes a drag; null for no snapping. */
  snap: { options: SnapOptions; targets: () => SnapTargets } | null;
}

/** The axis-aligned bounds of a box after a move or scale. */
function transformBounds(box: Bounds, by: Transform): Bounds {
  const corners = [
    { x: box.minX, y: box.minY },
    { x: box.maxX, y: box.maxY },
  ].map((corner) => applyToPoint(by, corner));
  return unionBounds(corners.map(({ x, y }) => ({ minX: x, minY: y, maxX: x, maxY: y })))!;
}

function once<T>(make: () => T): () => T {
  let value: { made: T } | null = null;
  return () => (value ??= { made: make() }).made;
}

const inside = (box: Bounds, { x, y }: Point) =>
  x >= box.minX && x <= box.maxX && y >= box.minY && y <= box.maxY;

let gestures = 0;

export class SelectTool {
  /** The marquee being dragged, in CSS px, for the overlay. */
  marquee = $state.raw<{ from: Point; to: Point; mode: MarqueeMode } | null>(null);
  /** Alignment guides for the drag under way, in world mm. */
  guides = $state.raw<Guide[]>([]);
  /** Equal-spacing hints for the move under way, in world mm. */
  gaps = $state.raw<Gap[]>([]);
  #press = $state.raw<Press | null>(null);
  #editor: DocumentEditor;
  #selection: Selection;
  #snap: () => SnapOptions;

  /** `snap` is read as each press starts. */
  constructor(
    editor: DocumentEditor,
    selection: Selection,
    snap: () => SnapOptions = () => SNAP_OFF,
  ) {
    this.#editor = editor;
    this.#selection = selection;
    this.#snap = snap;
  }

  /** A transform drag is under way. */
  get transforming(): boolean {
    const press = this.#press;
    return press !== null && press.dragging && press.gesture !== "marquee";
  }

  /** Handles for the current selection box, in CSS px. */
  handles(view: View, scene: Scene): Handle[] {
    const bounds = scene.boundsOf(this.#selection.ids);
    return bounds ? selectionHandles(view, bounds) : [];
  }

  /** Starts a press with the primary button. */
  down(input: PointerInput, view: View, scene: Scene): void {
    const world = screenToWorld(view, input.at);
    const bounds = scene.boundsOf(this.#selection.ids);
    const handle = bounds ? handleAt(selectionHandles(view, bounds), input.at) : null;
    let gesture: Press["gesture"] = "marquee";
    const hit = scene.hitTest(world, PICK_REACH / view.zoom);
    if (handle) {
      // Handles win over the outlines under them; a click without a drag
      // still picks the object (see `up`).
      gesture = handle.kind === "scale" ? { kind: "scale", handle: handle.id } : { kind: "rotate" };
    } else {
      if (hit !== null) {
        if (input.shift) this.#selection.toggle(hit);
        else if (!this.#selection.has(hit)) this.#selection.set([hit]);
        gesture = this.#selection.has(hit) ? { kind: "move" } : null;
      } else if (!input.shift && bounds && inside(bounds, world)) {
        // Anywhere inside the selection box drags it.
        gesture = { kind: "move" };
      }
    }
    const ids = this.#selection.ids;
    const options = this.#snap();
    const snaps =
      options.enabled && gesture !== null && gesture !== "marquee" && gesture.kind !== "rotate";
    this.#press = {
      start: input.at,
      startWorld: world,
      gesture,
      hit,
      additive: input.shift,
      dragging: false,
      last: input,
      doc: this.#editor.doc,
      ids,
      bounds: scene.boundsOf(ids),
      coalesce: `select-tool-${++gestures}`,
      snap: snaps ? { options, targets: once(() => dragTargets(scene, options, ids)) } : null,
    };
  }

  move(input: PointerInput, view: View): void {
    const press = this.#press;
    if (!press) return;
    const travel = Math.hypot(input.at.x - press.start.x, input.at.y - press.start.y);
    if (!press.dragging && travel < DRAG_THRESHOLD) return;
    this.#press = { ...press, dragging: true, last: input };
    this.#update(view);
  }

  /** Re-applies the drag when shift or alt changes without the pointer moving. */
  modifiers(modifiers: Modifiers, view: View): void {
    const press = this.#press;
    if (!press?.dragging) return;
    this.#press = { ...press, last: { ...press.last, ...modifiers } };
    this.#update(view);
  }

  up(view: View, scene: Scene): void {
    const press = this.#press;
    if (!press) return;
    this.#press = null;
    this.marquee = null;
    this.guides = [];
    this.gaps = [];
    if (press.gesture === "marquee") {
      if (press.dragging) {
        const ids = marqueeSelect(scene, this.#worldBox(press, view), this.#mode(press));
        if (press.additive) this.#selection.add(ids);
        else this.#selection.set(ids);
      } else if (!press.additive) {
        this.#selection.clear();
      }
    } else if (press.dragging) {
      if (press.gesture) this.#editor.endCoalesce();
    } else if (press.gesture?.kind === "move") {
      // A click on empty space inside the selection box still deselects.
      if (press.hit === null) this.#selection.clear();
    } else if (press.gesture && press.hit !== null) {
      // A click on a handle that sits over an outline picks it like any click.
      if (press.additive) this.#selection.toggle(press.hit);
      else if (!this.#selection.has(press.hit)) this.#selection.set([press.hit]);
    }
  }

  /** Drops an unfinished marquee. Transform drags carry on: undo reverts them. */
  cancel(): void {
    if (this.#press?.gesture === "marquee") {
      this.#press = null;
      this.marquee = null;
    }
  }

  /** The CSS cursor for the pointer at `at`, or while dragging. */
  cursor(at: Point | null, view: View, scene: Scene): string {
    const press = this.#press;
    if (press?.dragging && press.gesture !== "marquee" && press.gesture !== null) {
      if (press.gesture.kind === "move") return "move";
      if (press.gesture.kind === "rotate") return "grabbing";
    }
    if (!at) return "default";
    const handle = handleAt(this.handles(view, scene), at);
    if (handle) return handleCursor(handle);
    const world = screenToWorld(view, at);
    const bounds = scene.boundsOf(this.#selection.ids);
    if (bounds && inside(bounds, world)) return "move";
    return scene.hitTest(world, PICK_REACH / view.zoom) === null ? "default" : "pointer";
  }

  #mode(press: Press): MarqueeMode {
    return marqueeMode(press.start, press.last.at);
  }

  #worldBox(press: Press, view: View): Bounds {
    const from = screenToWorld(view, press.start);
    const to = screenToWorld(view, press.last.at);
    return {
      minX: Math.min(from.x, to.x),
      minY: Math.min(from.y, to.y),
      maxX: Math.max(from.x, to.x),
      maxY: Math.max(from.y, to.y),
    };
  }

  #update(view: View): void {
    const press = this.#press;
    if (!press?.gesture) return;
    if (press.gesture === "marquee") {
      this.marquee = { from: press.start, to: press.last.at, mode: this.#mode(press) };
      return;
    }
    if (!press.bounds || press.ids.length === 0) return;
    const gesture = press.gesture;
    const bounds = press.bounds;
    let to = screenToWorld(view, press.last.at);
    const snap = press.last.ctrl ? null : press.snap;
    if (snap) {
      // Shift the pointer so the box edge or handle lands on what it snaps to.
      const settings = snapSettings(view.zoom, snap.options);
      const dx = to.x - press.startWorld.x;
      const dy = to.y - press.startWorld.y;
      if (gesture.kind === "move") {
        const moved = {
          minX: bounds.minX + dx,
          minY: bounds.minY + dy,
          maxX: bounds.maxX + dx,
          maxY: bounds.maxY + dy,
        };
        const shift = snapBox(snap.targets(), moved, settings);
        to = { x: to.x + shift.x, y: to.y + shift.y };
      } else if (gesture.kind === "scale") {
        const anchor = HANDLE_ANCHORS[gesture.handle];
        const start = anchorPoint(bounds, anchor);
        const handle = { x: start.x + dx, y: start.y + dy };
        const snapped = snapPoint(snap.targets(), handle, settings, {
          x: anchor.x !== 0.5,
          y: anchor.y !== 0.5,
        });
        to = { x: to.x + snapped.x - handle.x, y: to.y + snapped.y - handle.y };
      }
    }
    const by = dragTransform(gesture, bounds, press.startWorld, to, press.last);
    const result = snap && gesture.kind !== "rotate" ? transformBounds(bounds, by) : null;
    this.guides = snap && result ? boxGuides(snap.targets(), result) : [];
    this.gaps =
      snap && result && gesture.kind === "move" ? spacingGaps(snap.targets(), result) : [];
    try {
      this.#editor.execute(transformObjects(press.doc, press.ids, by), {
        coalesce: press.coalesce,
      });
    } catch (error) {
      // An undo mid-drag can remove what the drag holds; the drag just stops.
      if (!(error instanceof DocumentError)) throw error;
      this.#press = null;
      this.guides = [];
      this.gaps = [];
      this.#editor.endCoalesce();
    }
  }
}
