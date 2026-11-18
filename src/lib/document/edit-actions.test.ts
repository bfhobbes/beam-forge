import { describe, expect, it } from "vite-plus/test";
import type { Action } from "#lib/actions/registry.svelte.js";
import { MemoryClipboard } from "#lib/desktop/clipboard.svelte.js";
import { Lifecycle } from "#lib/desktop/lifecycle.svelte.js";
import { insert } from "./commands.js";
import { DUPLICATE_OFFSET, editActions } from "./edit-actions.js";
import { DocumentEditor } from "./editor.svelte.js";
import { emptyDocument, findPlacement, type Point, type SceneObject } from "./model.js";
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
  const selection = new Selection(editor);
  const clipboard = new MemoryClipboard();
  const pointer: { at: Point | null } = { at: null };
  const list = editActions(editor, selection, { clipboard, pasteTarget: () => pointer.at });
  const action = (id: string): Action => {
    const found = list.find((candidate) => candidate.id === id);
    if (!found) throw new Error(`no action ${id}`);
    return found;
  };
  const enabled = (id: string) => action(id).enabled?.() ?? true;
  const transformOf = (id: string) => findPlacement(editor.doc.objects, id)?.object.transform;
  return { editor, selection, clipboard, pointer, action, enabled, transformOf };
}

/** Lets the clipboard's promises settle: paste reads it asynchronously. */
const settle = () => new Promise((resolve) => setTimeout(resolve, 0));

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

  it("copies and pastes in place as a new, selected object, one undo step", async () => {
    const { editor, selection, action, enabled, transformOf } = setup();
    expect(enabled("edit.copy")).toBe(false);
    expect(enabled("edit.paste-in-place")).toBe(false);
    selection.set(["a"]);
    action("edit.copy").run();
    await settle();
    expect(enabled("edit.paste-in-place")).toBe(true);
    action("edit.paste-in-place").run();
    await settle();
    expect(editor.doc.objects).toHaveLength(3);
    const [pasted] = selection.ids;
    expect(pasted).not.toBe("a");
    expect(transformOf(pasted)).toEqual(IDENTITY);
    editor.undo();
    expect(editor.doc.objects).toHaveLength(2);
  });

  it("pastes centred on the pointer, or in place when it was never over the canvas", async () => {
    const { selection, pointer, action, transformOf } = setup();
    selection.set(["a"]);
    action("edit.copy").run();
    await settle();
    action("edit.paste").run();
    await settle();
    expect(transformOf(selection.ids[0])).toEqual(IDENTITY);
    // The 10 × 5 rectangle's centre (5, 2.5) moves to the pointer.
    pointer.at = { x: 100, y: 50 };
    action("edit.paste").run();
    await settle();
    expect(transformOf(selection.ids[0])).toEqual(translate(95, 47.5));
  });

  it("cuts as one undo step, and the copy still pastes", async () => {
    const { editor, selection, action } = setup();
    selection.set(["a", "b"]);
    action("edit.cut").run();
    expect(editor.doc.objects).toEqual([]);
    expect(selection.ids).toEqual([]);
    await settle();
    action("edit.paste-in-place").run();
    await settle();
    expect(editor.doc.objects).toHaveLength(2);
    expect(selection.ids).toHaveLength(2);
    editor.undo();
    editor.undo();
    expect(editor.doc.objects.map((object) => object.id)).toEqual(["a", "b"]);
  });

  it("pastes into a newly loaded document and selects what it pasted", async () => {
    const { editor, selection, action } = setup();
    selection.set(["a"]);
    action("edit.copy").run();
    await settle();
    editor.load({ ...emptyDocument(), objects: [rect("a")] });
    expect(selection.ids).toEqual([]);
    action("edit.paste-in-place").run();
    await settle();
    expect(editor.doc.objects).toHaveLength(2);
    expect(selection.ids).toEqual([editor.doc.objects[1].id]);
  });

  it("ignores clipboard text that is not objects", async () => {
    const { editor, clipboard, action } = setup();
    await clipboard.write("some text from elsewhere");
    action("edit.paste").run();
    await settle();
    expect(editor.doc.objects).toHaveLength(2);
  });

  it("duplicates the selection, offset, as one undo step", () => {
    const { editor, selection, action, enabled, transformOf } = setup();
    expect(enabled("edit.duplicate")).toBe(false);
    selection.set(["b"]);
    action("edit.duplicate").run();
    const [copy] = selection.ids;
    expect(editor.doc.objects.map((object) => object.id)).toEqual(["a", "b", copy]);
    expect(transformOf(copy)).toEqual(translate(DUPLICATE_OFFSET.x, DUPLICATE_OFFSET.y));
    editor.undo();
    expect(editor.doc.objects).toHaveLength(2);
  });
});
