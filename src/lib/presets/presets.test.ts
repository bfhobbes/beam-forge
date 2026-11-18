import { describe, expect, it } from "vite-plus/test";
import { Lifecycle } from "#lib/desktop/lifecycle.svelte.js";
import { DocumentEditor } from "#lib/document/editor.svelte.js";
import { newLayer } from "#lib/document/layers.js";
import { emptyDocument } from "#lib/document/model.js";
import { editLayerSetting } from "#lib/shell/layers-panel.js";
import {
  applyPreset,
  defaultOperation,
  layerSettings,
  presetModified,
  presetName,
  searchPresets,
  type MaterialPreset,
} from "./presets.js";

const plywood: MaterialPreset = {
  id: "starter-plywood-3mm-cut",
  material: "Plywood",
  thickness: 3,
  settings: {
    operation: "cut",
    speed: 300,
    maxPower: 100,
    minPower: 0,
    passes: 1,
    airAssist: true,
  },
  starter: true,
  notes: "Starting point for a 20 W diode laser, not a tested value",
};

const basswood: MaterialPreset = {
  id: "starter-basswood-engrave",
  material: "Basswood",
  thickness: null,
  settings: {
    operation: "engraveFill",
    speed: 3000,
    maxPower: 30,
    minPower: 0,
    passes: 1,
    lineInterval: 0.08,
    airAssist: false,
  },
  starter: true,
  notes: "",
};

function editor() {
  return new DocumentEditor(new Lifecycle(), { ...emptyDocument(), layers: [newLayer(0)] });
}

describe("preset names and search", () => {
  it("names presets by thickness, material and operation", () => {
    expect(presetName(plywood)).toBe("3 mm Plywood · Cut");
    expect(presetName(basswood)).toBe("Basswood · Engrave");
    expect(presetName({ ...plywood, thickness: 1.5 })).toBe("1.5 mm Plywood · Cut");
  });

  it("finds presets by every word of the query, in name or notes", () => {
    const all = [plywood, basswood];
    expect(searchPresets(all, "")).toEqual(all);
    expect(searchPresets(all, "ply cut")).toEqual([plywood]);
    expect(searchPresets(all, "ENGRAVE")).toEqual([basswood]);
    expect(searchPresets(all, "tested")).toEqual([plywood]);
    expect(searchPresets(all, "ply engrave")).toEqual([]);
  });
});

describe("applying a preset to a layer", () => {
  it("sets the layer's settings and records the preset, in one undo step", () => {
    const doc = editor();
    expect(editLayerSetting(doc, "layer-1", applyPreset(basswood), false)).toBeNull();
    const layer = doc.doc.layers[0];
    expect(layer).toMatchObject({
      mode: "fill",
      speed: 3000,
      maxPower: 30,
      lineInterval: 0.08,
      airAssist: false,
      preset: { id: basswood.id, name: "Basswood · Engrave", settings: basswood.settings },
    });
    expect(presetModified(layer)).toBe(false);

    doc.undo();
    expect(doc.doc.layers[0]).toEqual(newLayer(0));
    expect(JSON.parse(JSON.stringify(doc.doc.layers[0]))).not.toHaveProperty("preset");
  });

  it("leaves the line interval alone for line presets", () => {
    const doc = editor();
    editLayerSetting(doc, "layer-1", { lineInterval: 0.2 }, false);
    editLayerSetting(doc, "layer-1", applyPreset(plywood), false);
    expect(doc.doc.layers[0]).toMatchObject({ mode: "line", speed: 300, lineInterval: 0.2 });
  });

  it("shows the layer as modified once a preset setting changes, and not for others", () => {
    const doc = editor();
    editLayerSetting(doc, "layer-1", applyPreset(plywood), false);
    editLayerSetting(doc, "layer-1", { name: "Outline", shown: false, lineInterval: 0.3 });
    expect(presetModified(doc.doc.layers[0])).toBe(false);

    for (const change of [
      { speed: 350 },
      { maxPower: 90 },
      { minPower: 5 },
      { passes: 2 },
      { airAssist: false },
      { mode: "fill" as const },
    ]) {
      editLayerSetting(doc, "layer-1", change, false);
      expect(presetModified(doc.doc.layers[0]), JSON.stringify(change)).toBe(true);
      doc.undo();
      expect(presetModified(doc.doc.layers[0])).toBe(false);
    }

    const fill = editor();
    editLayerSetting(fill, "layer-1", applyPreset(basswood), false);
    editLayerSetting(fill, "layer-1", { lineInterval: 0.1 });
    expect(presetModified(fill.doc.layers[0])).toBe(true);
  });

  it("is never modified without a preset", () => {
    expect(presetModified(newLayer(0))).toBe(false);
  });
});

describe("saving a layer as a preset", () => {
  it("takes the layer's settings, with the interval only for engraving", () => {
    const layer = { ...newLayer(0), speed: 800, maxPower: 70, passes: 2, lineInterval: 0.05 };
    expect(layerSettings(layer, "score")).toEqual({
      operation: "score",
      speed: 800,
      maxPower: 70,
      minPower: 0,
      passes: 2,
      airAssist: true,
    });
    expect(layerSettings({ ...layer, mode: "fill" }, "engraveFill").lineInterval).toBe(0.05);
  });

  it("suggests an operation that fits the layer's mode", () => {
    const layer = newLayer(0);
    expect(defaultOperation(layer)).toBe("cut");
    expect(defaultOperation({ ...layer, mode: "fill" })).toBe("engraveFill");
    const scored = {
      ...layer,
      preset: {
        id: "p",
        name: "Score",
        settings: { ...plywood.settings, operation: "score" as const },
      },
    };
    expect(defaultOperation(scored)).toBe("score");
    expect(defaultOperation({ ...scored, mode: "fill" })).toBe("engraveFill");
  });

  it("an applied preset saved back unchanged is not modified", () => {
    const doc = editor();
    editLayerSetting(doc, "layer-1", applyPreset(plywood), false);
    editLayerSetting(doc, "layer-1", { speed: 250 });
    const layer = doc.doc.layers[0];
    const updated = { ...plywood, settings: layerSettings(layer, defaultOperation(layer)) };
    editLayerSetting(doc, "layer-1", {
      preset: { id: updated.id, name: presetName(updated), settings: updated.settings },
    });
    expect(presetModified(doc.doc.layers[0])).toBe(false);
    expect(doc.doc.layers[0].speed).toBe(250);
  });
});
