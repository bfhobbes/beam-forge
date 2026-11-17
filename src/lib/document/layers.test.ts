import { describe, expect, it } from "vite-plus/test";
import { Lifecycle } from "#lib/desktop/lifecycle.svelte.js";
import {
  addLayer,
  applyCommand,
  assignLayer,
  compose,
  DocumentError,
  editLayer,
  insert,
  moveLayer,
  removeLayer,
  setObjectLayers,
  type Command,
} from "./commands.js";
import { DocumentEditor } from "./editor.svelte.js";
import {
  intervalToLpi,
  LAYER_PALETTE,
  layerForSlot,
  layerProblem,
  layerUsage,
  lpiToInterval,
  newLayer,
} from "./layers.js";
import { emptyDocument, findPlacement, type DesignDocument, type SceneObject } from "./model.js";
import { IDENTITY } from "./transform.js";

function rect(id: string, layerId = "layer-1"): SceneObject {
  return {
    type: "rect",
    id,
    name: id,
    layerId,
    transform: IDENTITY,
    width: 10,
    height: 5,
    cornerRadius: 0,
  };
}

/** Blue (layer-1) and red (layer-2); a rect and a group of two, all on blue. */
function sample(): DesignDocument {
  return {
    ...emptyDocument(),
    layers: [newLayer(0), newLayer(1)],
    objects: [
      rect("a"),
      {
        type: "group",
        id: "g",
        name: "g",
        layerId: "layer-1",
        transform: IDENTITY,
        children: [rect("b"), rect("c")],
      },
    ],
  };
}

/** Applies a command, checks its inverse restores the document, and returns the result. */
function roundTrip(doc: DesignDocument, command: Command): DesignDocument {
  const { doc: next, inverse } = applyCommand(doc, command);
  const { doc: back, inverse: redo } = applyCommand(next, inverse);
  expect(back).toEqual(doc);
  expect(applyCommand(back, redo).doc).toEqual(next);
  return next;
}

const layerIds = (doc: DesignDocument) => doc.layers.map((layer) => layer.id);
const layerOf = (doc: DesignDocument, id: string) => findPlacement(doc.objects, id)?.object.layerId;

describe("layer commands", () => {
  it("adds layers at the end or at an index, and the inverse removes them", () => {
    const doc = sample();
    expect(layerIds(roundTrip(doc, addLayer(newLayer(2))))).toEqual([
      "layer-1",
      "layer-2",
      "layer-3",
    ]);
    expect(layerIds(roundTrip(doc, addLayer(newLayer(2), 0)))).toEqual([
      "layer-3",
      "layer-1",
      "layer-2",
    ]);
  });

  it("refuses duplicate layer ids, bad settings and bad indexes", () => {
    const doc = sample();
    expect(() => applyCommand(doc, addLayer(newLayer(0)))).toThrow(DocumentError);
    expect(() => applyCommand(doc, addLayer({ ...newLayer(2), passes: 0 }))).toThrow(/Passes/);
    expect(() => applyCommand(doc, addLayer(newLayer(2), 5))).toThrow(DocumentError);
  });

  it("removes only layers nothing is on, and the inverse puts them back in place", () => {
    const doc = sample();
    expect(layerIds(roundTrip(doc, removeLayer("layer-2")))).toEqual(["layer-1"]);
    expect(() => applyCommand(doc, removeLayer("layer-1"))).toThrow(/still has objects/);
    expect(() => applyCommand(doc, removeLayer("nope"))).toThrow(DocumentError);
  });

  it("reorders layers, which is the cut order", () => {
    const doc = roundTrip(sample(), addLayer(newLayer(2)));
    expect(layerIds(roundTrip(doc, moveLayer("layer-3", 0)))).toEqual([
      "layer-3",
      "layer-1",
      "layer-2",
    ]);
    expect(layerIds(roundTrip(doc, moveLayer("layer-1", 2)))).toEqual([
      "layer-2",
      "layer-3",
      "layer-1",
    ]);
    expect(() => applyCommand(doc, moveLayer("layer-1", 3))).toThrow(DocumentError);
  });

  it("edits settings, and the inverse restores only what changed", () => {
    const doc = sample();
    const edited = roundTrip(doc, editLayer("layer-2", { speed: 3000, mode: "fill" }));
    expect(edited.layers[1]).toMatchObject({ speed: 3000, mode: "fill", maxPower: 50 });
    expect(applyCommand(doc, editLayer("layer-2", { speed: 3000 })).inverse).toEqual(
      editLayer("layer-2", { speed: 1000 }),
    );
  });

  it("refuses settings a layer can't be cut with", () => {
    const doc = sample();
    expect(() => applyCommand(doc, editLayer("layer-1", { minPower: 60 }))).toThrow(/Minimum/);
    expect(() => applyCommand(doc, editLayer("layer-1", { speed: -1 }))).toThrow(/Speed/);
    expect(() => applyCommand(doc, editLayer("nope", { speed: 1 }))).toThrow(DocumentError);
  });

  it("assigns objects to a layer, groups with everything in them", () => {
    const doc = sample();
    const moved = roundTrip(doc, assignLayer(doc, ["g"], "layer-2"));
    expect(["a", "g", "b", "c"].map((id) => layerOf(moved, id))).toEqual([
      "layer-1",
      "layer-2",
      "layer-2",
      "layer-2",
    ]);
  });

  it("undoes an assignment back to each object's own layer", () => {
    const doc = roundTrip(sample(), setObjectLayers({ c: "layer-2" }));
    const assigned = applyCommand(doc, assignLayer(doc, ["a", "g"], "layer-2"));
    expect(["a", "g", "b", "c"].map((id) => layerOf(assigned.doc, id))).toEqual(
      Array(4).fill("layer-2"),
    );
    const undone = applyCommand(assigned.doc, assigned.inverse).doc;
    expect(["a", "g", "b", "c"].map((id) => layerOf(undone, id))).toEqual([
      "layer-1",
      "layer-1",
      "layer-1",
      "layer-2",
    ]);
  });

  it("refuses to assign to a missing layer or a missing object", () => {
    const doc = sample();
    expect(() => applyCommand(doc, setObjectLayers({ a: "nope" }))).toThrow(DocumentError);
    expect(() => assignLayer(doc, ["nope"], "layer-2")).toThrow(DocumentError);
  });

  it("merges successive edits of one layer, not of different layers", () => {
    const merged = compose(editLayer("layer-1", { speed: 1 }), editLayer("layer-1", { speed: 2 }));
    expect(merged).toEqual(editLayer("layer-1", { speed: 2 }));
    const separate = compose(
      editLayer("layer-1", { speed: 1 }),
      editLayer("layer-2", { speed: 2 }),
    );
    expect(separate.type).toBe("batch");
  });
});

describe("layer edits in the editor", () => {
  it("coalesces repeated edits of one field into one undo step", () => {
    const editor = new DocumentEditor(new Lifecycle(), sample());
    for (const speed of [1100, 1200, 1300]) {
      editor.execute(editLayer("layer-1", { speed }), { coalesce: "layer-1:speed" });
    }
    editor.execute(editLayer("layer-1", { passes: 3 }), { coalesce: "layer-1:passes" });
    expect(editor.doc.layers[0]).toMatchObject({ speed: 1300, passes: 3 });
    editor.undo();
    expect(editor.doc.layers[0]).toMatchObject({ speed: 1300, passes: 1 });
    editor.undo();
    expect(editor.doc.layers[0]).toMatchObject({ speed: 1000, passes: 1 });
    expect(editor.canUndo).toBe(false);
  });

  it("undoes and redoes assignment and reordering", () => {
    const editor = new DocumentEditor(new Lifecycle(), sample());
    editor.execute(assignLayer(editor.doc, ["a"], "layer-2"));
    editor.execute(moveLayer("layer-2", 0));
    editor.execute(insert([rect("d", "layer-2")]));
    expect(layerIds(editor.doc)).toEqual(["layer-2", "layer-1"]);
    editor.undo();
    editor.undo();
    expect(layerIds(editor.doc)).toEqual(["layer-1", "layer-2"]);
    editor.undo();
    expect(layerOf(editor.doc, "a")).toBe("layer-1");
    editor.redo();
    expect(layerOf(editor.doc, "a")).toBe("layer-2");
  });
});

describe("layer helpers", () => {
  it("has 30 distinct palette colours, each with a valid default layer", () => {
    expect(LAYER_PALETTE).toHaveLength(30);
    expect(new Set(LAYER_PALETTE).size).toBe(30);
    for (const slot of LAYER_PALETTE.keys()) expect(layerProblem(newLayer(slot))).toBeNull();
  });

  it("picks an unused id for a palette colour", () => {
    const doc = sample();
    expect(layerForSlot(doc, 2).id).toBe("layer-3");
    expect(layerForSlot(doc, 0)).toMatchObject({ id: "layer-1-2", color: "#0000ff" });
  });

  it("counts the objects on each layer", () => {
    expect(layerUsage(sample().objects)).toEqual(new Map([["layer-1", 4]]));
  });

  it("converts lines per inch to a line interval and back", () => {
    expect(lpiToInterval(254)).toBeCloseTo(0.1);
    expect(intervalToLpi(0.1)).toBeCloseTo(254);
  });
});
