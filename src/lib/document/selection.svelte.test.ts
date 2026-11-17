import { describe, expect, it } from "vite-plus/test";
import { Lifecycle } from "#lib/desktop/lifecycle.svelte.js";
import { insert, remove } from "./commands.js";
import { DocumentEditor } from "./editor.svelte.js";
import type { SceneObject } from "./model.js";
import { Selection } from "./selection.svelte.js";
import { IDENTITY } from "./transform.js";

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
  editor.execute(insert([rect("a"), rect("b"), rect("c")]));
  return { editor, selection: new Selection(() => editor.doc) };
}

describe("selection", () => {
  it("replaces, toggles, adds and clears", () => {
    const { selection } = setup();
    selection.set(["a"]);
    expect(selection.ids).toEqual(["a"]);
    selection.toggle("b");
    expect(selection.ids).toEqual(["a", "b"]);
    selection.toggle("a");
    expect(selection.ids).toEqual(["b"]);
    selection.add(["b", "c"]);
    expect(selection.ids).toEqual(["b", "c"]);
    expect(selection.has("c")).toBe(true);
    selection.clear();
    expect(selection.ids).toEqual([]);
  });

  it("selects every top-level object", () => {
    const { selection } = setup();
    selection.selectAll();
    expect(selection.ids).toEqual(["a", "b", "c"]);
  });

  it("drops objects that leave the document and brings them back on undo", () => {
    const { editor, selection } = setup();
    selection.set(["a", "b"]);
    editor.execute(remove(["a"]));
    expect(selection.ids).toEqual(["b"]);
    editor.undo();
    expect(selection.ids).toEqual(["a", "b"]);
  });
});
