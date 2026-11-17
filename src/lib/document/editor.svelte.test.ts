import { describe, expect, it } from "vite-plus/test";
import { Lifecycle } from "#lib/desktop/lifecycle.svelte.js";
import { group, insert, remove, setTransforms, ungroup } from "./commands.js";
import { DocumentEditor } from "./editor.svelte.js";
import { findPlacement, type SceneObject } from "./model.js";
import { IDENTITY, translate } from "./transform.js";

function rect(id: string): SceneObject {
  return {
    type: "rect",
    id,
    name: id,
    layerId: "layer-1",
    transform: IDENTITY,
    width: 10,
    height: 5,
    cornerRadius: 0,
  };
}

function setup() {
  const lifecycle = new Lifecycle();
  const editor = new DocumentEditor(lifecycle);
  return { lifecycle, editor };
}

function transformOf(editor: DocumentEditor, id: string) {
  return findPlacement(editor.doc.objects, id)?.object.transform;
}

describe("document editor", () => {
  it("starts empty and clean with nothing to undo", () => {
    const { lifecycle, editor } = setup();
    expect(editor.doc.objects).toEqual([]);
    expect(editor.canUndo).toBe(false);
    expect(editor.canRedo).toBe(false);
    expect(lifecycle.dirty).toBe(false);
  });

  it("undoes and redoes add, remove, transform, group and ungroup", () => {
    const { editor } = setup();
    const steps = [
      insert([rect("a"), rect("b"), rect("c")]),
      setTransforms({ a: translate(3, 4) }),
      group(["a", "b"], "g"),
      ungroup(["g"]),
      remove(["c"]),
    ];
    const snapshots = [editor.doc];
    for (const step of steps) {
      expect(editor.execute(step)).toBe(true);
      snapshots.push(editor.doc);
    }
    for (let i = steps.length - 1; i >= 0; i--) {
      expect(editor.undo()).toBe(true);
      expect(editor.doc).toEqual(snapshots[i]);
    }
    expect(editor.undo()).toBe(false);
    for (let i = 1; i <= steps.length; i++) {
      expect(editor.redo()).toBe(true);
      expect(editor.doc).toEqual(snapshots[i]);
    }
    expect(editor.redo()).toBe(false);
  });

  it("clears redo when a new command runs after an undo", () => {
    const { editor } = setup();
    editor.execute(insert([rect("a")]));
    editor.execute(insert([rect("b")]));
    editor.undo();
    editor.execute(insert([rect("c")]));
    expect(editor.canRedo).toBe(false);
  });

  it("marks the document dirty on every change", () => {
    const { lifecycle, editor } = setup();
    editor.execute(insert([rect("a")]));
    expect(lifecycle.dirty).toBe(true);
    lifecycle.dirty = false;
    editor.undo();
    expect(lifecycle.dirty).toBe(true);
    lifecycle.dirty = false;
    editor.redo();
    expect(lifecycle.dirty).toBe(true);
  });

  it("refuses edits, undo and redo while the lifecycle forbids editing", () => {
    const { lifecycle, editor } = setup();
    editor.execute(insert([rect("a")]));
    editor.execute(insert([rect("b")]));
    editor.undo();
    lifecycle.dirty = false;
    const unlock = lifecycle.acquireInstallLock();
    const before = editor.doc;
    expect(editor.execute(insert([rect("c")]))).toBe(false);
    expect(editor.undo()).toBe(false);
    expect(editor.redo()).toBe(false);
    expect(editor.doc).toBe(before);
    expect(lifecycle.dirty).toBe(false);
    unlock?.();
    expect(editor.redo()).toBe(true);
  });

  it("leaves the document and history unchanged when a command is invalid", () => {
    const { editor } = setup();
    editor.execute(insert([rect("a")]));
    const before = editor.doc;
    expect(() => editor.execute(remove(["missing"]))).toThrow();
    expect(editor.doc).toBe(before);
    editor.undo();
    expect(editor.doc.objects).toEqual([]);
  });

  it("coalesces a 100-update drag into one undo step", () => {
    const { editor } = setup();
    editor.execute(insert([rect("a"), rect("b")]));
    for (let i = 1; i <= 100; i++) {
      editor.execute(setTransforms({ a: translate(i, 0), b: translate(0, i) }), {
        coalesce: "drag",
      });
    }
    editor.endCoalesce();
    expect(transformOf(editor, "a")).toEqual(translate(100, 0));
    editor.undo();
    expect(transformOf(editor, "a")).toEqual(IDENTITY);
    expect(transformOf(editor, "b")).toEqual(IDENTITY);
    expect(editor.doc.objects).toHaveLength(2);
    editor.redo();
    expect(transformOf(editor, "a")).toEqual(translate(100, 0));
    expect(transformOf(editor, "b")).toEqual(translate(0, 100));
  });

  it("coalesces commands of different kinds into one step", () => {
    const { editor } = setup();
    editor.execute(insert([rect("a")]), { coalesce: "draw" });
    editor.execute(setTransforms({ a: translate(1, 1) }), { coalesce: "draw" });
    editor.execute(setTransforms({ a: translate(2, 2) }), { coalesce: "draw" });
    editor.undo();
    expect(editor.doc.objects).toEqual([]);
    editor.redo();
    expect(transformOf(editor, "a")).toEqual(translate(2, 2));
  });

  it("starts a new step after endCoalesce, a different key, or an undo", () => {
    const { editor } = setup();
    editor.execute(insert([rect("a")]));
    editor.execute(setTransforms({ a: translate(1, 0) }), { coalesce: "drag" });
    editor.endCoalesce();
    editor.execute(setTransforms({ a: translate(2, 0) }), { coalesce: "drag" });
    editor.execute(setTransforms({ a: translate(3, 0) }), { coalesce: "nudge" });
    editor.undo();
    expect(transformOf(editor, "a")).toEqual(translate(2, 0));
    editor.execute(setTransforms({ a: translate(9, 0) }), { coalesce: "drag" });
    editor.undo();
    expect(transformOf(editor, "a")).toEqual(translate(2, 0));
    editor.undo();
    expect(transformOf(editor, "a")).toEqual(translate(1, 0));
  });

  it("loads a document with fresh history and a clean state", () => {
    const { lifecycle, editor } = setup();
    editor.execute(insert([rect("a")]));
    const loaded = { version: 1 as const, units: "mm" as const, layers: [], objects: [rect("z")] };
    editor.load(loaded);
    expect(editor.doc).toEqual(loaded);
    expect(editor.canUndo).toBe(false);
    expect(lifecycle.dirty).toBe(false);
  });
});
