import { describe, expect, it } from "vite-plus/test";
import type { Action } from "#lib/actions/registry.svelte.js";
import { selectionBounds } from "#lib/canvas/arrange.js";
import { Lifecycle } from "#lib/desktop/lifecycle.svelte.js";
import { arrangeActions } from "./arrange-actions.js";
import { insert } from "./commands.js";
import { DocumentEditor } from "./editor.svelte.js";
import type { SceneObject, TextObject } from "./model.js";
import { Selection } from "./selection.svelte.js";
import { IDENTITY, translate } from "./transform.js";

const rect = (id: string, x: number, y: number): SceneObject => ({
  type: "rect",
  id,
  name: id,
  layerId: "layer-1",
  transform: translate(x, y),
  width: 10,
  height: 5,
  cornerRadius: 0,
});

const text: TextObject = {
  type: "text",
  id: "t",
  name: "Text",
  layerId: "layer-1",
  transform: translate(50, 50),
  text: "Hi",
  fontFamily: "Arial",
  fontWeight: 400,
  italic: false,
  height: 10,
  letterSpacing: 0,
  lineSpacing: 1,
  align: "left",
  outline: [
    {
      start: { x: 0, y: 0 },
      segments: [
        { kind: "line", to: { x: 4, y: 0 } },
        { kind: "line", to: { x: 4, y: 7 } },
      ],
      closed: true,
    },
  ],
};

const imported: SceneObject = {
  type: "path",
  id: "p",
  name: "Imported",
  layerId: "layer-1",
  transform: IDENTITY,
  subpaths: [
    {
      start: { x: 0, y: 0 },
      segments: [
        { kind: "cubic", control1: { x: 5, y: 0 }, control2: { x: 5, y: 5 }, to: { x: 0, y: 5 } },
      ],
      closed: false,
    },
  ],
};

function setup(...objects: SceneObject[]) {
  const editor = new DocumentEditor(new Lifecycle());
  editor.execute(insert(objects));
  const selection = new Selection(editor);
  const list = arrangeActions(editor, selection, () => ({ width: 400, height: 300 }));
  const action = (id: string): Action => {
    const found = list.find((candidate) => candidate.id === id);
    if (!found) throw new Error(`no action ${id}`);
    return found;
  };
  const enabled = (id: string) => action(id).enabled?.() ?? true;
  const ids = () => editor.doc.objects.map((object) => object.id);
  const bounds = (id: string) => selectionBounds(editor.doc, [id])!;
  return { editor, selection, action, enabled, ids, bounds };
}

describe("arrange actions", () => {
  it("are all in the Arrange group with unique ids", () => {
    const { editor, selection } = setup();
    const list = arrangeActions(editor, selection, () => ({ width: 1, height: 1 }));
    expect(new Set(list.map((action) => action.id)).size).toBe(list.length);
    expect(list.every((action) => action.group === "Arrange")).toBe(true);
  });

  it("groups and ungroups, keeping text outlines and imported paths, each one undo step", () => {
    const { editor, selection, action, enabled, ids, bounds } = setup(
      rect("a", 0, 0),
      text,
      imported,
    );
    const before = editor.doc;
    expect(enabled("edit.group")).toBe(false);
    selection.set(["t", "p"]);
    action("edit.group").run();
    expect(ids()).toHaveLength(2);
    const [groupId] = selection.ids;
    const created = editor.doc.objects[1];
    expect(created.id).toBe(groupId);
    if (created.type !== "group") throw new Error("expected a group");
    expect(created.children).toEqual([text, imported]);

    // Moving the group then ungrouping bakes the move into its members.
    action("arrange.align-left-bed").run();
    const moved = bounds(groupId);
    action("edit.ungroup").run();
    expect(ids()).toEqual(["a", "t", "p"]);
    expect(selection.ids.toSorted()).toEqual(["p", "t"]);
    const ungrouped = editor.doc.objects[1] as TextObject;
    expect(ungrouped.outline).toEqual(text.outline);
    expect(ungrouped.text).toBe("Hi");
    expect(selectionBounds(editor.doc, ["t", "p"])).toEqual(moved);

    editor.undo();
    editor.undo();
    editor.undo();
    expect(editor.doc).toEqual(before);
  });

  it("aligns to the selection or the bed", () => {
    const { editor, selection, action, enabled, bounds } = setup(
      rect("a", 0, 0),
      rect("b", 30, 20),
    );
    selection.set(["a"]);
    expect(enabled("arrange.align-left")).toBe(false);
    expect(enabled("arrange.align-top-bed")).toBe(true);
    action("arrange.align-top-bed").run();
    expect(bounds("a").maxY).toBe(300);
    selection.set(["a", "b"]);
    action("arrange.align-right").run();
    expect(bounds("a").maxX).toBe(40);
    action("arrange.align-centre-bed").run();
    expect(bounds("a").minX).toBe(195);
    expect(bounds("b").minX).toBe(195);
    editor.undo();
    expect(bounds("b").minX).toBe(30);
  });

  it("distributes three or more", () => {
    const { selection, action, enabled, bounds } = setup(
      rect("a", 0, 0),
      rect("b", 15, 0),
      rect("c", 100, 0),
    );
    selection.set(["a", "b"]);
    expect(enabled("arrange.distribute-horizontal")).toBe(false);
    selection.set(["a", "b", "c"]);
    action("arrange.distribute-horizontal").run();
    expect(bounds("b").minX).toBe(50);
  });

  it("changes stacking order, disabled when the selection is already there", () => {
    const { editor, selection, action, enabled, ids } = setup(
      rect("a", 0, 0),
      rect("b", 0, 0),
      rect("c", 0, 0),
    );
    selection.set(["c"]);
    expect(enabled("arrange.bring-forward")).toBe(false);
    expect(enabled("arrange.bring-to-front")).toBe(false);
    action("arrange.send-backward").run();
    expect(ids()).toEqual(["a", "c", "b"]);
    action("arrange.send-to-back").run();
    expect(ids()).toEqual(["c", "a", "b"]);
    expect(selection.ids).toEqual(["c"]);
    action("arrange.bring-to-front").run();
    expect(ids()).toEqual(["a", "b", "c"]);
    editor.undo();
    expect(ids()).toEqual(["c", "a", "b"]);
  });

  it("mirrors in place", () => {
    const { editor, selection, action, bounds } = setup(rect("a", 10, 10));
    selection.set(["a"]);
    action("arrange.mirror-horizontal").run();
    expect(bounds("a")).toEqual({ minX: 10, minY: 10, maxX: 20, maxY: 15 });
    expect(editor.doc.objects[0].transform[0]).toBe(-1);
    action("arrange.mirror-vertical").run();
    expect(editor.doc.objects[0].transform[3]).toBe(-1);
    editor.undo();
    editor.undo();
    expect(editor.doc.objects[0].transform).toEqual(translate(10, 10));
  });
});
