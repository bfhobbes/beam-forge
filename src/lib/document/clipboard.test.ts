import { describe, expect, it } from "vite-plus/test";
import { applyCommand } from "./commands.js";
import {
  CLIPBOARD_FORMAT,
  copyContent,
  duplicateCommand,
  parseContent,
  pasteCommand,
  serializeContent,
} from "./clipboard.js";
import { newLayer } from "./layers.js";
import { allIds, emptyDocument, type DesignDocument, type SceneObject } from "./model.js";
import { IDENTITY, translate } from "./transform.js";

const text: SceneObject = {
  type: "text",
  id: "t",
  name: "Text",
  layerId: "layer-2",
  transform: translate(10, 20),
  text: "Hi",
  fontFamily: "Arial",
  fontWeight: 400,
  italic: false,
  height: 10,
  letterSpacing: 0,
  lineSpacing: 1,
  align: "left",
  outline: [
    { start: { x: 0, y: 0 }, segments: [{ kind: "line", to: { x: 1, y: 1 } }], closed: false },
  ],
};

const path: SceneObject = {
  type: "path",
  id: "p",
  name: "Imported",
  layerId: "layer-1",
  transform: IDENTITY,
  subpaths: [
    {
      start: { x: 0, y: 0 },
      segments: [
        { kind: "cubic", control1: { x: 1, y: 0 }, control2: { x: 1, y: 1 }, to: { x: 0, y: 1 } },
      ],
      closed: true,
    },
  ],
};

const grouped: SceneObject = {
  type: "group",
  id: "g",
  name: "Group",
  layerId: "layer-1",
  transform: translate(5, 0),
  children: [text, path],
};

function source(): DesignDocument {
  return {
    ...emptyDocument(),
    layers: [newLayer(0), newLayer(1), newLayer(2)],
    objects: [grouped, { ...path, id: "lone", layerId: "layer-3" }],
  };
}

describe("clipboard", () => {
  it("copies the selected objects with only the layers they use, and reads them back", () => {
    const content = copyContent(source(), ["g"]);
    expect(content.objects).toEqual([grouped]);
    expect(content.layers.map((layer) => layer.id)).toEqual(["layer-1", "layer-2"]);
    expect(parseContent(serializeContent(content))).toEqual(content);
  });

  it("ignores text that is not Beam Forge objects", () => {
    expect(parseContent("hello")).toBeNull();
    expect(parseContent("{}")).toBeNull();
    expect(
      parseContent(
        JSON.stringify({ format: CLIPBOARD_FORMAT, version: 1, objects: [], layers: [] }),
      ),
    ).toBeNull();
    expect(
      parseContent(
        JSON.stringify({
          format: CLIPBOARD_FORMAT,
          version: 1,
          objects: [{ id: "x" }],
          layers: [],
        }),
      ),
    ).toBeNull();
  });

  it("pastes into another document with fresh ids, its layers, outlines and paths intact", () => {
    const content = copyContent(source(), ["g"]);
    const target: DesignDocument = {
      ...emptyDocument(),
      layers: [{ ...newLayer(0), name: "Mine" }],
    };
    const { command, ids } = pasteCommand(target, content, { x: 1, y: 2 });
    const { doc, inverse } = applyCommand(target, command);
    // The target's own layer-1 stays; layer-2 comes from the clipboard.
    expect(doc.layers.map((layer) => [layer.id, layer.name])).toEqual([
      ["layer-1", "Mine"],
      ["layer-2", "Layer 2"],
    ]);
    expect(doc.objects.map((object) => object.id)).toEqual(ids);
    const pasted = doc.objects[0];
    if (pasted.type !== "group") throw new Error("expected a group");
    expect(pasted.transform).toEqual(translate(6, 2));
    expect(new Set(allIds([pasted])).has("t")).toBe(false);
    expect(pasted.children).toHaveLength(2);
    const [pastedText, pastedPath] = pasted.children;
    expect(pastedText).toEqual({ ...text, id: pastedText.id });
    expect(pastedPath).toEqual({ ...path, id: pastedPath.id });
    expect(applyCommand(doc, inverse).doc).toEqual(target);
  });

  it("pastes twice without clashing ids", () => {
    const doc = source();
    const content = copyContent(doc, ["lone"]);
    const once = applyCommand(doc, pasteCommand(doc, content).command).doc;
    const twice = applyCommand(once, pasteCommand(once, content).command).doc;
    expect(twice.objects).toHaveLength(4);
    expect(new Set(allIds(twice.objects)).size).toBe([...allIds(twice.objects)].length);
  });

  it("duplicates on top, moved by the offset", () => {
    const doc = source();
    const { command, ids } = duplicateCommand(doc, ["g"], { x: 10, y: -10 });
    const next = applyCommand(doc, command).doc;
    expect(next.objects.map((object) => object.id)).toEqual(["g", "lone", ids[0]]);
    expect(next.objects[2].transform).toEqual(translate(15, -10));
    expect(next.layers).toEqual(doc.layers);
  });
});
