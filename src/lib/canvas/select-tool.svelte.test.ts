import { describe, expect, it } from "vite-plus/test";
import { Lifecycle } from "#lib/desktop/lifecycle.svelte.js";
import { insert } from "#lib/document/commands.js";
import { DocumentEditor } from "#lib/document/editor.svelte.js";
import { findPlacement, type SceneObject } from "#lib/document/model.js";
import { Selection } from "#lib/document/selection.svelte.js";
import { IDENTITY, translate, type Transform } from "#lib/document/transform.js";
import { buildScene } from "./scene.js";
import { SelectTool } from "./select-tool.svelte.js";
import type { View } from "./view.js";

const rect = (id: string, transform: Transform = IDENTITY): SceneObject => ({
  type: "rect",
  id,
  name: id,
  layerId: "layer-1",
  transform,
  width: 10,
  height: 5,
  cornerRadius: 0,
});

// 2 px per mm, world origin at screen (0, 200): a is x 0–20, y 190–200 on screen.
const view: View = { zoom: 2, x: 0, y: 200 };

function setup() {
  const editor = new DocumentEditor(new Lifecycle());
  editor.execute(insert([rect("a"), rect("b", translate(50, 0))]));
  editor.load(editor.doc); // a clean history
  const selection = new Selection(editor);
  const tool = new SelectTool(editor, selection);
  const scene = () => buildScene(editor.doc);
  const press = (x: number, y: number, shift = false, alt = false) =>
    tool.down({ at: { x, y }, shift, alt }, view, scene());
  const drag = (x: number, y: number, shift = false, alt = false) =>
    tool.move({ at: { x, y }, shift, alt }, view);
  const release = () => tool.up(view, scene());
  const click = (x: number, y: number, shift = false) => {
    press(x, y, shift);
    release();
  };
  const transformOf = (id: string) => findPlacement(editor.doc.objects, id)?.object.transform;
  return { editor, selection, tool, press, drag, release, click, transformOf };
}

describe("select tool", () => {
  it("selects the clicked object and clears on empty space", () => {
    const { selection, click, editor } = setup();
    click(0, 195);
    expect(selection.ids).toEqual(["a"]);
    click(100, 195);
    expect(selection.ids).toEqual(["b"]);
    click(60, 100);
    expect(selection.ids).toEqual([]);
    expect(editor.canUndo).toBe(false);
  });

  it("toggles with shift-click", () => {
    const { selection, click } = setup();
    click(0, 195);
    click(100, 195, true);
    expect(selection.ids).toEqual(["a", "b"]);
    click(0, 195, true);
    expect(selection.ids).toEqual(["b"]);
  });

  it("moves the selection by dragging as one undo step", () => {
    const { editor, press, drag, release, transformOf } = setup();
    press(0, 195);
    drag(10, 195);
    drag(20, 185);
    release();
    expect(transformOf("a")).toEqual(translate(10, 5));
    expect(transformOf("b")).toEqual(translate(50, 0));
    editor.undo();
    expect(transformOf("a")).toEqual(IDENTITY);
    expect(editor.canUndo).toBe(false);
  });

  it("ignores a tiny wobble during a click", () => {
    const { editor, press, drag, release } = setup();
    press(0, 195);
    drag(1, 196);
    release();
    expect(editor.canUndo).toBe(false);
  });

  it("selects with a window marquee left to right, crossing right to left", () => {
    const { selection, tool, press, drag, release } = setup();
    // Covers all of a and the left part of b.
    press(-10, 180);
    drag(110, 210);
    expect(tool.marquee?.mode).toBe("window");
    release();
    expect(selection.ids).toEqual(["a"]);
    expect(tool.marquee).toBeNull();

    press(110, 180);
    drag(-10, 210);
    expect(tool.marquee?.mode).toBe("crossing");
    release();
    expect(selection.ids).toEqual(["a", "b"]);
  });

  it("adds a marquee to the selection with shift", () => {
    const { selection, click, press, drag, release } = setup();
    click(100, 195);
    press(-10, 180, true);
    drag(30, 210, true);
    release();
    expect(selection.ids).toEqual(["b", "a"]);
  });

  it("scales from a corner handle as one undo step", () => {
    const { editor, click, press, drag, release, transformOf } = setup();
    click(0, 195);
    // The north-east handle sits on a's top-right corner, screen (20, 190).
    press(20, 190);
    drag(30, 190);
    drag(40, 180);
    release();
    expect(transformOf("a")).toEqual([2, 0, 0, 2, 0, 0]);
    editor.undo();
    expect(transformOf("a")).toEqual(IDENTITY);
    expect(editor.canUndo).toBe(false);
  });

  it("re-applies the drag when shift changes mid-drag", () => {
    const { tool, click, press, drag, release, transformOf } = setup();
    click(0, 195);
    press(20, 190);
    drag(40, 190);
    expect(transformOf("a")).toEqual([2, 0, 0, 1, 0, 0]);
    tool.modifiers({ shift: true, alt: false }, view);
    expect(transformOf("a")).toEqual([2, 0, 0, 2, 0, 0]);
    release();
  });

  it("rotates from a rotate handle", () => {
    const { click, press, drag, release, transformOf } = setup();
    click(0, 195);
    // The north-east rotate handle is out from the corner along the diagonal.
    const offset = 18 / Math.SQRT2;
    press(20 + offset, 190 - offset);
    drag(0, 300);
    release();
    const [a, b] = transformOf("a")!;
    expect(Math.abs(Math.atan2(b, a))).toBeGreaterThan(0.1);
  });

  it("drops an unfinished marquee on cancel", () => {
    const { selection, tool, press, drag, release } = setup();
    press(-10, 180);
    drag(110, 210);
    tool.cancel();
    expect(tool.marquee).toBeNull();
    release();
    expect(selection.ids).toEqual([]);
  });
});
