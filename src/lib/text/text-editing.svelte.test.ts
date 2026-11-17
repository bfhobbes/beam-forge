import { describe, expect, it } from "vite-plus/test";
import { Lifecycle } from "#lib/desktop/lifecycle.svelte.js";
import { DrawingLayer } from "#lib/document/drawing-layer.svelte.js";
import { DocumentEditor } from "#lib/document/editor.svelte.js";
import type { TextObject } from "#lib/document/model.js";
import { Selection } from "#lib/document/selection.svelte.js";
import { previewLayout, type TextAdapter, type TextLayout } from "./adapter.js";
import { defaultFamily, TextEditing } from "./text-editing.svelte.js";

/** The preview layout, but each call waits until the test releases it. */
function slowAdapter() {
  const waiting: (() => void)[] = [];
  const adapter: TextAdapter = {
    available: true,
    fonts: () => Promise.resolve([]),
    layout: (text, style) =>
      new Promise<TextLayout>((resolve) => waiting.push(() => resolve(previewLayout(text, style)))),
  };
  return { adapter, release: (i: number) => waiting[i]() };
}

function setup(adapter: TextAdapter = slowAdapter().adapter) {
  const editor = new DocumentEditor(new Lifecycle());
  const selection = new Selection(() => editor.doc);
  const editing = new TextEditing(editor, selection, new DrawingLayer(), adapter);
  return { editor, selection, editing };
}

const instant: TextAdapter = {
  available: true,
  fonts: () => Promise.resolve([]),
  layout: (text, style) => Promise.resolve(previewLayout(text, style)),
};

const texts = (editor: DocumentEditor) =>
  editor.doc.objects.filter((object): object is TextObject => object.type === "text");

describe("typing text on the canvas", () => {
  it("adds the text on the first keystroke and edits it in place after", async () => {
    const { editor, selection, editing } = setup(instant);
    editing.create({ x: 50, y: 60 });
    expect(texts(editor)).toHaveLength(0);

    await editing.input("H", 1, 1);
    const [added] = texts(editor);
    expect(added).toMatchObject({ text: "H", transform: [1, 0, 0, 1, 50, 60], height: 10 });
    expect(added.outline).toHaveLength(1);
    expect(selection.ids).toEqual([added.id]);
    expect(editor.doc.layers).toHaveLength(1);

    await editing.input("Hi", 2, 2);
    await editing.input("Hi\nyo", 5, 5);
    expect(texts(editor)).toHaveLength(1);
    expect(texts(editor)[0].text).toBe("Hi\nyo");
    expect(texts(editor)[0].outline).toHaveLength(4);

    // The whole session is one undo step, layer included.
    editing.finish();
    editor.undo();
    expect(editor.doc.objects).toHaveLength(0);
    expect(editor.doc.layers).toHaveLength(0);
  });

  it("leaves nothing behind when nothing is typed, and removes text emptied out", async () => {
    const { editor, editing } = setup(instant);
    editing.create({ x: 0, y: 0 });
    editing.finish();
    expect(editor.doc.objects).toHaveLength(0);

    editing.create({ x: 0, y: 0 });
    await editing.input("A", 1, 1);
    const id = texts(editor)[0].id;
    editing.finish();
    editing.edit(id);
    await editing.input("", 0, 0);
    editing.finish();
    expect(editor.doc.objects).toHaveLength(0);
  });

  it("drops a layout that a later keystroke overtook", async () => {
    const { adapter, release } = slowAdapter();
    const { editor, editing } = setup(adapter);
    editing.create({ x: 0, y: 0 });
    release(0); // The caret layout for the empty text.
    const first = editing.input("A", 1, 1);
    const second = editing.input("AB", 2, 2);
    release(2);
    await second;
    release(1);
    await first;
    expect(texts(editor)[0].text).toBe("AB");
  });

  it("restyles with a fresh outline as one undo step, and remembers the style", async () => {
    const { editor, editing } = setup(instant);
    editing.create({ x: 0, y: 0 });
    await editing.input("AB", 2, 2);
    editing.finish();
    const id = texts(editor)[0].id;
    expect(await editing.restyle(id, { height: 20 })).toBe(true);
    const [restyled] = texts(editor);
    expect(restyled.height).toBe(20);
    expect(restyled.outline[0].segments[1].to.y).toBeCloseTo(20);
    expect(editing.defaults.height).toBe(20);
    editor.undo();
    expect(texts(editor)[0].height).toBe(10);
  });

  it("draws the caret on the right line and spans a selection", async () => {
    const { editing } = setup(instant);
    editing.create({ x: 100, y: 0 });
    await editing.input("ab\ncd", 4, 4);
    const caret = editing.caret();
    // Second line, after "c": 1 advance (0.6 × 10 mm) along, one line down.
    expect(caret?.caret?.from.x).toBeCloseTo(106);
    expect(caret?.caret?.from.y).toBeLessThan(-10);

    editing.select(1, 4);
    const span = editing.caret();
    expect(span?.caret).toBeNull();
    expect(span?.selection).toHaveLength(2);
  });
});

describe("default family", () => {
  it("prefers a common sans, else the first family", () => {
    expect(defaultFamily(["Zapfino", "arial", "Comic Sans MS"])).toBe("arial");
    expect(defaultFamily(["DejaVu Sans", "Liberation Sans"])).toBe("Liberation Sans");
    expect(defaultFamily(["Zapfino"])).toBe("Zapfino");
    expect(defaultFamily([])).toBeNull();
  });
});
