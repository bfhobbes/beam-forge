import { describe, expect, it } from "vite-plus/test";
import type { Action } from "#lib/actions/registry.svelte.js";
import { Lifecycle } from "#lib/desktop/lifecycle.svelte.js";
import { insert } from "./commands.js";
import { editActions } from "./edit-actions.js";
import { DocumentEditor } from "./editor.svelte.js";
import { findPlacement, type SceneObject } from "./model.js";
import { Selection } from "./selection.svelte.js";
import { IDENTITY, translate } from "./transform.js";

const rect = (id: string): SceneObject => ({
  type: "rect",
  id,
  name: id,
  layerId: "layer-1",
  transform: IDENTITY,
  width: 10,
  height: 5,
  cornerRadius: 0,
});

function setup() {
  const editor = new DocumentEditor(new Lifecycle());
  editor.execute(insert([rect("a"), rect("b")]));
  const selection = new Selection(() => editor.doc);
  const list = editActions(editor, selection);
  const action = (id: string): Action => {
    const found = list.find((candidate) => candidate.id === id);
    if (!found) throw new Error(`no action ${id}`);
    return found;
  };
  const enabled = (id: string) => action(id).enabled?.() ?? true;
  const transformOf = (id: string) => findPlacement(editor.doc.objects, id)?.object.transform;
  return { editor, selection, action, enabled, transformOf };
}

describe("edit actions", () => {
  it("undoes and redoes, enabled only when there is a step", () => {
    const { editor, action, enabled } = setup();
    expect(enabled("edit.undo")).toBe(true);
    expect(enabled("edit.redo")).toBe(false);
    action("edit.undo").run();
    expect(editor.doc.objects).toEqual([]);
    expect(enabled("edit.redo")).toBe(true);
    action("edit.redo").run();
    expect(editor.doc.objects).toHaveLength(2);
  });

  it("selects all and clears", () => {
    const { selection, action } = setup();
    action("edit.select-all").run();
    expect(selection.ids).toEqual(["a", "b"]);
    action("edit.deselect").run();
    expect(selection.ids).toEqual([]);
  });

  it("deletes the selection as one undo step", () => {
    const { editor, selection, action, enabled } = setup();
    expect(enabled("edit.delete")).toBe(false);
    selection.set(["a", "b"]);
    action("edit.delete").run();
    expect(editor.doc.objects).toEqual([]);
    expect(selection.ids).toEqual([]);
    editor.undo();
    expect(editor.doc.objects).toHaveLength(2);
  });

  it("nudges by 1 mm, or 10 mm with shift, one undo step per press", () => {
    const { editor, selection, action, enabled, transformOf } = setup();
    expect(enabled("edit.nudge-right")).toBe(false);
    selection.set(["a"]);
    action("edit.nudge-right").run();
    action("edit.nudge-up").run();
    expect(transformOf("a")).toEqual(translate(1, 1));
    action("edit.nudge-left-far").run();
    action("edit.nudge-down-far").run();
    expect(transformOf("a")).toEqual(translate(-9, -9));
    expect(transformOf("b")).toEqual(IDENTITY);
    editor.undo();
    expect(transformOf("a")).toEqual(translate(-9, 1));
  });

  it("converts selected shapes to paths in one undo step", () => {
    const { editor, selection, action, enabled } = setup();
    expect(enabled("edit.convert-to-path")).toBe(false);
    selection.set(["a"]);
    expect(enabled("edit.convert-to-path")).toBe(true);
    action("edit.convert-to-path").run();
    expect(editor.doc.objects.map((object) => object.type)).toEqual(["path", "rect"]);
    expect(selection.ids).toEqual(["a"]);
    expect(enabled("edit.convert-to-path")).toBe(false);
    editor.undo();
    expect(editor.doc.objects.map((object) => object.type)).toEqual(["rect", "rect"]);
  });
});
