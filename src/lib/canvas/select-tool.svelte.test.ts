import { describe, expect, it } from "vite-plus/test";
import { Lifecycle } from "#lib/desktop/lifecycle.svelte.js";
import { insert } from "#lib/document/commands.js";
import { DocumentEditor } from "#lib/document/editor.svelte.js";
import { findPlacement, type SceneObject } from "#lib/document/model.js";
import { Selection } from "#lib/document/selection.svelte.js";
import { IDENTITY, translate, type Transform } from "#lib/document/transform.js";
import { buildScene } from "./scene.js";
import { SelectTool } from "./select-tool.svelte.js";
import type { SnapOptions } from "./snap.js";
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

const SNAP: SnapOptions = { enabled: true, grid: false, bed: null, area: null };

function setup(snap?: SnapOptions) {
  const editor = new DocumentEditor(new Lifecycle());
  editor.execute(insert([rect("a"), rect("b", translate(50, 0))]));
  editor.load(editor.doc); // a clean history
  const selection = new Selection(editor);
  const tool = new SelectTool(editor, selection, snap && (() => snap));
  const scene = () => buildScene(editor.doc);
  const press = (x: number, y: number, shift = false, alt = false) =>
    tool.down({ at: { x, y }, shift, alt }, view, scene());
  const drag = (x: number, y: number, shift = false, alt = false, ctrl = false) =>
    tool.move({ at: { x, y }, shift, alt, ctrl }, view);
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

describe("select tool snapping", () => {
  // Reach is 6 px: 3 mm at 2 px/mm.
  it("snaps a moved box onto another's edge and shows the guide", () => {
    const { tool, press, drag, release, transformOf } = setup(SNAP);
    press(0, 195);
    // 38.5 mm right puts a's right edge at 48.5, 1.5 mm short of b's left edge.
    drag(77, 195);
    expect(transformOf("a")).toEqual(translate(40, 0));
    expect(tool.guides).toContainEqual({ axis: "x", at: 50, from: 0, to: 5 });
    release();
    expect(tool.guides).toEqual([]);
  });

  it("holds off while ctrl is down, and snaps again when it is let go", () => {
    const { tool, press, drag, release, transformOf } = setup(SNAP);
    press(0, 195);
    drag(77, 195, false, false, true);
    expect(transformOf("a")).toEqual(translate(38.5, 0));
    expect(tool.guides).toEqual([]);
    tool.modifiers({ shift: false, alt: false, ctrl: false }, view);
    expect(transformOf("a")).toEqual(translate(40, 0));
    release();
  });

  it("does not snap with snapping off", () => {
    const { press, drag, release, transformOf } = setup();
    press(0, 195);
    drag(77, 195);
    release();
    expect(transformOf("a")).toEqual(translate(38.5, 0));
  });

  it("snaps the dragged edge when resizing", () => {
    const { tool, click, press, drag, release, transformOf } = setup(SNAP);
    click(0, 195);
    // The east handle at screen (20, 195) dragged to x = 48.8 mm snaps to b's left edge at 50.
    press(20, 195);
    drag(97.6, 195);
    expect(transformOf("a")).toEqual([5, 0, 0, 1, 0, 0]);
    expect(tool.guides).toContainEqual({ axis: "x", at: 50, from: 0, to: 5 });
    release();
  });

  it("snaps to the bed and the grid", () => {
    const { press, drag, release, transformOf } = setup({
      ...SNAP,
      grid: true,
      bed: { minX: 0, minY: 0, maxX: 400, maxY: 300 },
    });
    press(0, 195);
    // a's edges to y = 11.3 and 16.3: no object or bed line near, so the 5 mm grid at 2 px/mm takes them down 1.3.
    drag(0, 172.4);
    expect(transformOf("a")).toEqual(translate(0, 10));
    // Its centre at x = 199 snaps to the bed's centre at 200.
    drag(388, 195);
    expect(transformOf("a")).toEqual(translate(195, 0));
    release();
  });

  it("snaps a moved box to an equal spacing and shows the gaps", () => {
    const { editor, tool, press, drag, release, transformOf } = setup(SNAP);
    editor.execute(insert([rect("c", translate(100, 0))]));
    press(0, 195);
    // a at 74.6–84.6 sits 0.4 mm short of midway between b (ends at 60) and c (starts at 100).
    drag(149.2, 195);
    expect(transformOf("a")).toEqual(translate(75, 0));
    expect(tool.gaps).toEqual([
      { axis: "x", from: 60, to: 75, at: 2.5 },
      { axis: "x", from: 85, to: 100, at: 2.5 },
    ]);
    release();
    expect(tool.gaps).toEqual([]);
  });
});
