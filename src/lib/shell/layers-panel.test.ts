import { describe, expect, it } from "vite-plus/test";
import { Lifecycle } from "#lib/desktop/lifecycle.svelte.js";
import { applyCommand } from "#lib/document/commands.js";
import { DocumentEditor } from "#lib/document/editor.svelte.js";
import { newLayer } from "#lib/document/layers.js";
import { emptyDocument, type DesignDocument } from "#lib/document/model.js";
import { IDENTITY } from "#lib/document/transform.js";
import { clickSwatch, constantPowerWarning, editLayerSetting } from "./layers-panel.js";

function sample(): DesignDocument {
  return {
    ...emptyDocument(),
    layers: [newLayer(0)],
    objects: [
      {
        type: "ellipse",
        id: "e",
        name: "e",
        layerId: "layer-1",
        transform: IDENTITY,
        rx: 1,
        ry: 1,
      },
    ],
  };
}

describe("layer palette swatches", () => {
  it("adds a layer when nothing is selected", () => {
    const { command, layerId } = clickSwatch(sample(), [], 1);
    expect(layerId).toBe("layer-2");
    const { doc } = applyCommand(sample(), command!);
    expect(doc.layers.map((layer) => layer.color)).toEqual(["#0000ff", "#ff0000"]);
  });

  it("only shows an existing layer when nothing is selected", () => {
    expect(clickSwatch(sample(), [], 0)).toEqual({ command: null, layerId: "layer-1" });
  });

  it("moves the selection to the colour's layer, adding it in the same undo step", () => {
    const { command, layerId } = clickSwatch(sample(), ["e"], 1);
    const { doc, inverse } = applyCommand(sample(), command!);
    expect(doc.objects[0].layerId).toBe(layerId);
    expect(applyCommand(doc, inverse).doc).toEqual(sample());
  });
});

describe("layer setting edits", () => {
  it("report why a value was refused and leave the document alone", () => {
    const editor = new DocumentEditor(new Lifecycle(), sample());
    expect(editLayerSetting(editor, "layer-1", { passes: 0 })).toMatch(/Passes/);
    expect(editor.canUndo).toBe(false);
    expect(editLayerSetting(editor, "layer-1", { passes: 2 })).toBeNull();
    expect(editor.doc.layers[0].passes).toBe(2);
  });

  it("coalesce per field", () => {
    const editor = new DocumentEditor(new Lifecycle(), sample());
    editLayerSetting(editor, "layer-1", { speed: 2000 });
    editLayerSetting(editor, "layer-1", { speed: 3000 });
    editLayerSetting(editor, "layer-1", { shown: false }, false);
    editor.undo();
    expect(editor.doc.layers[0]).toMatchObject({ speed: 3000, shown: true });
    editor.undo();
    expect(editor.doc.layers[0].speed).toBe(1000);
    expect(editor.canUndo).toBe(false);
  });
});

describe("constant power warning", () => {
  it("warns about a dropped connection when the layer is on M3", () => {
    const warning = constantPowerWarning({ ...newLayer(0), constantPower: true });
    expect(warning).toContain("If the USB connection drops during this layer");
    expect(warning).toContain("until the machine is reset or switched off");
    expect(warning).toContain("Use dynamic power");
  });

  it("is quiet for dynamic power (M4), the default", () => {
    expect(constantPowerWarning(newLayer(0))).toBeNull();
  });
});
