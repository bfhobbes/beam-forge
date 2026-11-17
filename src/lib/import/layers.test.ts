import { describe, expect, it } from "vite-plus/test";
import { LAYER_PALETTE, newLayer } from "#lib/document/layers.js";
import { emptyDocument, type DesignDocument } from "#lib/document/model.js";
import { layersForColors } from "./layers.js";

const withLayers = (...slots: number[]): DesignDocument => ({
  ...emptyDocument(),
  layers: slots.map((slot) => newLayer(slot)),
});

describe("colour to layer mapping", () => {
  it("puts palette colours on their own layer, new or existing", () => {
    const { layers, added } = layersForColors(["#ff0000", "#0000ff", "#fe0101"], withLayers(1));
    expect(layers.get("#ff0000")).toBe("layer-2");
    expect(layers.get("#fe0101")).toBe("layer-2");
    expect(layers.get("#0000ff")).toBe("layer-1");
    expect(added.map((layer) => [layer.id, layer.color])).toEqual([["layer-1", "#0000ff"]]);
  });

  it("reuses an existing layer that shows the colour, whatever its id", () => {
    const doc = { ...emptyDocument(), layers: [{ ...newLayer(1), id: "C07" }] };
    const { layers, added } = layersForColors(["#ff0000"], doc);
    expect(layers.get("#ff0000")).toBe("C07");
    expect(added).toEqual([]);
  });

  it("gives other colours the next unused palette colours", () => {
    const { layers, added } = layersForColors(
      ["#000000", "#7a3c9e", "#0000ff", "#000000"],
      withLayers(1),
    );
    expect(layers.get("#0000ff")).toBe("layer-1");
    expect(layers.get("#000000")).toBe("layer-3");
    expect(layers.get("#7a3c9e")).toBe("layer-4");
    expect(layers.size).toBe(3);
    expect(added.map((layer) => layer.id)).toEqual(["layer-1", "layer-3", "layer-4"]);
  });

  it("shares the nearest colour's layer once the palette is used up", () => {
    const doc = withLayers(...LAYER_PALETTE.keys());
    const { layers, added } = layersForColors(["#2020c0"], doc);
    expect(added).toEqual([]);
    expect(layers.get("#2020c0")).toBe("layer-9");
  });

  it("never reuses a taken id for a new layer", () => {
    const doc = { ...emptyDocument(), layers: [{ ...newLayer(1), id: "layer-1" }] };
    const { layers, added } = layersForColors(["#0000ff"], doc);
    expect(layers.get("#0000ff")).toBe("layer-1-2");
    expect(added[0].color).toBe("#0000ff");
  });
});
