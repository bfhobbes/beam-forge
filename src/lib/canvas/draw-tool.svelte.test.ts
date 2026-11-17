import { describe, expect, it } from "vite-plus/test";
import { Lifecycle } from "#lib/desktop/lifecycle.svelte.js";
import { addLayer } from "#lib/document/commands.js";
import { DrawingLayer } from "#lib/document/drawing-layer.svelte.js";
import { DocumentEditor } from "#lib/document/editor.svelte.js";
import { newLayer } from "#lib/document/layers.js";
import { Selection } from "#lib/document/selection.svelte.js";
import type { ShapeTool } from "./draw.js";
import { DrawTool } from "./draw-tool.svelte.js";
import type { View } from "./view.js";

// 2 px per mm, world origin at screen (0, 200): world (x, y) is screen (2x, 200 - 2y).
const view: View = { zoom: 2, x: 0, y: 200 };
const screen = (x: number, y: number) => ({ x: 2 * x, y: 200 - 2 * y });

function setup(layers = [newLayer(0), newLayer(1)]) {
  const editor = new DocumentEditor(new Lifecycle());
  for (const layer of layers) editor.execute(addLayer(layer));
  editor.load(editor.doc); // a clean history
  const selection = new Selection(editor);
  const drawingLayer = new DrawingLayer();
  let done = 0;
  const tool = new DrawTool(editor, selection, drawingLayer, () => done++);
  const input = (x: number, y: number, shift = false, alt = false) => ({
    at: screen(x, y),
    shift,
    alt,
  });
  const dragShape = (shape: ShapeTool, from: [number, number], to: [number, number]) => {
    tool.down(input(...from), view, shape);
    tool.move(input(...to), view);
    tool.up(view);
  };
  const click = (x: number, y: number) => {
    tool.down(input(x, y), view, "line");
    tool.up(view);
  };
  return {
    editor,
    selection,
    drawingLayer,
    tool,
    input,
    dragShape,
    click,
    done: () => done,
  };
}

describe("draw tool", () => {
  it.each([
    ["rectangle", "rect"],
    ["ellipse", "ellipse"],
    ["polygon", "polygon"],
  ] as const)("a %s drag creates the shape, selected, as one undo step", (shape, type) => {
    const { editor, selection, dragShape, done } = setup();
    dragShape(shape, [10, 10], [30, 25]);
    expect(editor.doc.objects).toHaveLength(1);
    const [object] = editor.doc.objects;
    expect(object.type).toBe(type);
    expect(object.layerId).toBe("layer-1");
    expect(selection.ids).toEqual([object.id]);
    expect(done()).toBe(1);
    editor.undo();
    expect(editor.doc.objects).toEqual([]);
    expect(editor.canUndo).toBe(false);
  });

  it("previews the shape while dragging and follows shift", () => {
    const { tool, input } = setup();
    tool.down(input(0, 0), view, "rectangle");
    tool.move(input(20, 10), view);
    expect(tool.draft).toHaveLength(1);
    const corner = () => tool.draft[0].segments[1].to;
    expect(corner()).toEqual({ x: 20, y: 10 });
    tool.modifiers({ shift: true, alt: false }, view);
    expect(corner()).toEqual({ x: 20, y: 20 });
    tool.up(view);
    expect(tool.draft).toEqual([]);
  });

  it("makes nothing from a click", () => {
    const { editor, dragShape, done } = setup();
    dragShape("rectangle", [10, 10], [10, 10]);
    expect(editor.doc.objects).toEqual([]);
    expect(done()).toBe(0);
  });

  it("draws on the layer picked in the Layers panel", () => {
    const { editor, drawingLayer, dragShape } = setup();
    drawingLayer.id = "layer-2";
    dragShape("ellipse", [0, 0], [10, 10]);
    expect(editor.doc.objects[0].layerId).toBe("layer-2");
  });

  it("adds a layer when the design has none, in the same undo step", () => {
    const { editor, dragShape } = setup([]);
    dragShape("rectangle", [0, 0], [10, 10]);
    expect(editor.doc.layers.map((layer) => layer.id)).toEqual(["layer-1"]);
    expect(editor.doc.objects[0].layerId).toBe("layer-1");
    editor.undo();
    expect(editor.doc.layers).toEqual([]);
  });

  it("a line takes clicked points and finishes open", () => {
    const { editor, selection, tool, click, input } = setup();
    click(0, 0);
    click(10, 0);
    tool.move(input(10, 10), view);
    // The rubber band runs to the pointer.
    expect(tool.draft[0].segments.map((segment) => segment.to)).toEqual([
      { x: 10, y: 0 },
      { x: 10, y: 10 },
    ]);
    click(10, 10);
    // A double-click's second click lands on the last point: no new point.
    click(10, 10);
    tool.finish();
    const [line] = editor.doc.objects;
    expect(line.type === "path" && line.subpaths).toEqual([
      {
        start: { x: 0, y: 0 },
        segments: [
          { kind: "line", to: { x: 10, y: 0 } },
          { kind: "line", to: { x: 10, y: 10 } },
        ],
        closed: false,
      },
    ]);
    expect(selection.ids).toEqual([line.id]);
    expect(tool.drawingLine).toBe(false);
  });

  it("a line closes when clicked back on its first point", () => {
    const { editor, click } = setup();
    click(0, 0);
    click(10, 0);
    click(10, 10);
    click(0.5, 0.5);
    const [shape] = editor.doc.objects;
    expect(shape.type === "path" && shape.subpaths[0].closed).toBe(true);
    expect(shape.type === "path" && shape.subpaths[0].segments).toHaveLength(2);
  });

  it("escape abandons a line, and one point finishes as nothing", () => {
    const { editor, tool, click, done } = setup();
    click(0, 0);
    click(10, 0);
    tool.cancel();
    expect(tool.drawingLine).toBe(false);
    expect(tool.draft).toEqual([]);
    click(0, 0);
    tool.finish();
    expect(editor.doc.objects).toEqual([]);
    expect(done()).toBe(0);
  });
});
