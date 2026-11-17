/**
 * The drawing tools' pointer gestures. Rectangle, ellipse and polygon are
 * drags; line clicks points and finishes on a double-click, Enter, or a
 * click back on its first point (which closes it). Escape abandons the
 * shape. A finished shape is one undo step, on the layer picked in the
 * Layers panel, and is left selected.
 */
import { batch, insert, type Command } from "#lib/document/commands.js";
import type { DrawingLayer } from "#lib/document/drawing-layer.svelte.js";
import type { DocumentEditor } from "#lib/document/editor.svelte.js";
import {
  newId,
  type PathObject,
  type Point,
  type SceneObject,
  type Subpath,
} from "#lib/document/model.js";
import type { Selection } from "#lib/document/selection.svelte.js";
import { outline, transformSubpath, type ShapeObject } from "#lib/document/shapes.js";
import type { Modifiers } from "./arrange.js";
import { pathFromPoints, shapeFromDrag, type ShapeDrag, type ShapeTool } from "./draw.js";
import type { PointerInput } from "./select-tool.svelte.js";
import { screenToWorld, worldToScreen, type View } from "./view.js";

/** How close a click must be to a line's first or last point to hit it, CSS px. */
const POINT_REACH = 6;

interface Drag {
  tool: Exclude<ShapeTool, "line">;
  from: Point;
  last: PointerInput;
}

const PLACEHOLDER = { id: "draft", layerId: "" };

export class DrawTool {
  /** The shape being drawn, in world mm, for the overlay. */
  draft = $state.raw<Subpath[]>([]);
  #drag: Drag | null = null;
  /** A line's points so far, world mm. */
  #points: Point[] = [];
  #hover: Point | null = null;
  #editor: DocumentEditor;
  #selection: Selection;
  #layer: DrawingLayer;
  #done: () => void;

  /** `done` runs after each finished shape. */
  constructor(
    editor: DocumentEditor,
    selection: Selection,
    layer: DrawingLayer,
    done: () => void = () => {},
  ) {
    this.#editor = editor;
    this.#selection = selection;
    this.#layer = layer;
    this.#done = done;
  }

  /** A drag is under way, so shift and alt change it. */
  get dragging(): boolean {
    return this.#drag !== null;
  }

  /** A line has points and is waiting for more. */
  get drawingLine(): boolean {
    return this.#points.length > 0;
  }

  down(input: PointerInput, view: View, tool: ShapeTool): void {
    const world = screenToWorld(view, input.at);
    if (tool === "line") {
      this.#click(input.at, world, view);
      return;
    }
    this.cancel();
    this.#drag = { tool, from: world, last: input };
  }

  move(input: PointerInput, view: View): void {
    if (this.#drag) {
      this.#drag = { ...this.#drag, last: input };
      this.#preview(view);
    } else if (this.#points.length > 0) {
      this.#hover = screenToWorld(view, input.at);
      this.#preview(view);
    }
  }

  modifiers(modifiers: Modifiers, view: View): void {
    if (!this.#drag) return;
    this.#drag = { ...this.#drag, last: { ...this.#drag.last, ...modifiers } };
    this.#preview(view);
  }

  up(view: View): void {
    const drag = this.#drag;
    if (!drag) return;
    this.#drag = null;
    this.draft = [];
    const shape = shapeFromDrag(drag.tool, this.#shapeDrag(drag, view), this.#newObject());
    if (shape) this.#commit(shape);
  }

  /** Finishes a line as it stands, open. */
  finish(): void {
    const points = this.#points;
    this.cancel();
    const path = pathFromPoints(points, false, this.#newObject());
    if (path) this.#commit(path);
  }

  /** Abandons the shape being drawn. */
  cancel(): void {
    this.#drag = null;
    this.#points = [];
    this.#hover = null;
    this.draft = [];
  }

  #click(at: Point, world: Point, view: View): void {
    const points = this.#points;
    const near = (point: Point | undefined) => {
      if (!point) return false;
      const screen = worldToScreen(view, point);
      return Math.hypot(screen.x - at.x, screen.y - at.y) <= POINT_REACH;
    };
    if (points.length >= 3 && near(points[0])) {
      this.cancel();
      const path = pathFromPoints(points, true, this.#newObject());
      if (path) this.#commit(path);
      return;
    }
    // A double-click's second click lands on the point the first one made.
    if (near(points.at(-1))) return;
    this.#points = [...points, world];
    this.#hover = world;
    this.#preview(view);
  }

  #shapeDrag(drag: Drag, view: View): ShapeDrag {
    return {
      from: drag.from,
      to: screenToWorld(view, drag.last.at),
      shift: drag.last.shift,
      alt: drag.last.alt,
    };
  }

  #preview(view: View): void {
    let object: ShapeObject | PathObject | null;
    if (this.#drag) {
      object = shapeFromDrag(this.#drag.tool, this.#shapeDrag(this.#drag, view), PLACEHOLDER);
    } else {
      const points = this.#hover ? [...this.#points, this.#hover] : this.#points;
      object = pathFromPoints(points, false, PLACEHOLDER);
    }
    this.draft = object
      ? outline(object).map((subpath) => transformSubpath(object.transform, subpath))
      : [];
  }

  #newObject() {
    return { id: newId(), layerId: "" };
  }

  #commit(object: SceneObject): void {
    const { layer, add } = this.#layer.forNewShape(this.#editor.doc);
    const placed = { ...object, layerId: layer.id };
    const steps: Command[] = add ? [add, insert([placed])] : [insert([placed])];
    if (!this.#editor.execute(steps.length === 1 ? steps[0] : batch(steps))) return;
    this.#selection.set([placed.id]);
    this.#done();
  }
}
