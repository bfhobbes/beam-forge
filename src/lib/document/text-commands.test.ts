import { describe, expect, it } from "vite-plus/test";
import { applyCommand, compose, convertToPath, DocumentError, editText } from "./commands.js";
import { emptyDocument, type DesignDocument, type Subpath, type TextObject } from "./model.js";
import { outline } from "./shapes.js";
import { translate } from "./transform.js";

const square = (x: number): Subpath => ({
  start: { x, y: 0 },
  segments: [
    { kind: "line", to: { x: x + 1, y: 0 } },
    { kind: "quad", control: { x: x + 1, y: 1 }, to: { x, y: 1 } },
  ],
  closed: true,
});

function text(id = "t"): TextObject {
  return {
    type: "text",
    id,
    name: "Text",
    layerId: "layer-1",
    transform: translate(10, 20),
    text: "Hi",
    fontFamily: "Arial",
    fontWeight: 400,
    italic: false,
    height: 10,
    letterSpacing: 0,
    lineSpacing: 1,
    align: "left",
    outline: [square(0), square(2)],
  };
}

const docWith = (...objects: TextObject[]): DesignDocument => ({ ...emptyDocument(), objects });

describe("text commands", () => {
  it("edits the string, style and outline together, and undoes exactly", () => {
    const doc = docWith(text());
    const command = editText("t", {
      text: "Hi!",
      fontWeight: 700,
      outline: [square(0), square(2), square(4)],
    });
    const { doc: next, inverse } = applyCommand(doc, command);
    expect(next.objects[0]).toMatchObject({ text: "Hi!", fontWeight: 700 });
    expect(outline(next.objects[0] as TextObject)).toHaveLength(3);
    expect(applyCommand(next, inverse).doc).toEqual(doc);
  });

  it("folds successive edits of one text into one undo step", () => {
    const typed = compose(editText("t", { text: "Hi " }), editText("t", { text: "Hi y" }));
    expect(typed).toEqual(editText("t", { text: "Hi y" }));
  });

  it("refuses impossible styles and objects that aren't text", () => {
    const doc = docWith(text());
    expect(() => applyCommand(doc, editText("t", { height: 0 }))).toThrow(DocumentError);
    expect(() => applyCommand(doc, editText("t", { lineSpacing: -1 }))).toThrow(DocumentError);
    expect(() => applyCommand(doc, editText("t", { fontWeight: 1001 }))).toThrow(DocumentError);
    const rect: DesignDocument = {
      ...emptyDocument(),
      objects: [{ ...text(), type: "rect", width: 1, height: 1, cornerRadius: 0 } as never],
    };
    expect(() => applyCommand(rect, editText("t", { text: "x" }))).toThrow(/is not text/);
  });

  it("converts to a path with exactly the same geometry", () => {
    const doc = docWith(text());
    const { doc: next } = applyCommand(doc, convertToPath(doc, ["t"]));
    expect(next.objects[0]).toEqual({
      type: "path",
      id: "t",
      name: "Text",
      layerId: "layer-1",
      transform: translate(10, 20),
      subpaths: text().outline,
    });
  });
});
