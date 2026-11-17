/**
 * The select tool's pointer gestures on the canvas: click and shift-click
 * select, marquee select, and dragging the selection or its handles. Each
 * drag is one undo step: every update executes with the gesture's coalesce
 * key, rebuilt from the document as it was when the drag began.
 */
import { DocumentError } from "#lib/document/commands.js";
import type { DocumentEditor } from "#lib/document/editor.svelte.js";
import type { DesignDocument, Point } from "#lib/document/model.js";
import type { Selection } from "#lib/document/selection.svelte.js";
import { dragTransform, transformObjects, type Gesture, type Modifiers } from "./arrange.js";
import { handleAt, handleCursor, selectionHandles, type Handle } from "./handles.js";
import { marqueeMode, marqueeSelect, type MarqueeMode } from "./marquee.js";
import type { Scene } from "./scene.js";
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
}

const inside = (box: Bounds, { x, y }: Point) =>
  x >= box.minX && x <= box.maxX && y >= box.minY && y <= box.maxY;

let gestures = 0;

export class SelectTool {
  /** The marquee being dragged, in CSS px, for the overlay. */
  marquee = $state.raw<{ from: Point; to: Point; mode: MarqueeMode } | null>(null);
  #press = $state.raw<Press | null>(null);
  #editor: DocumentEditor;
  #selection: Selection;

  constructor(editor: DocumentEditor, selection: Selection) {
    this.#editor = editor;
    this.#selection = selection;
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
    const by = dragTransform(
      press.gesture,
      press.bounds,
      press.startWorld,
      screenToWorld(view, press.last.at),
      press.last,
    );
    try {
      this.#editor.execute(transformObjects(press.doc, press.ids, by), {
        coalesce: press.coalesce,
      });
    } catch (error) {
      // An undo mid-drag can remove what the drag holds; the drag just stops.
      if (!(error instanceof DocumentError)) throw error;
      this.#press = null;
      this.#editor.endCoalesce();
    }
  }
}
