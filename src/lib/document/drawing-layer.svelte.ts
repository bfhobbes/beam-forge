import { addLayer } from "./commands.js";
import type { Command } from "./commands.js";
import { layerForSlot } from "./layers.js";
import type { CutLayer, DesignDocument } from "./model.js";

/**
 * The layer picked in the Layers panel (the current colour): the
 * panel edits it and new shapes are drawn on it. Editor state, not part of
 * the document.
 */
export class DrawingLayer {
  /** The picked layer's id; null (or a layer that's gone) means the first. */
  id = $state<string | null>(null);

  /** The picked layer, else the first in cut order, else none. */
  current(doc: DesignDocument): CutLayer | undefined {
    return doc.layers.find((layer) => layer.id === this.id) ?? doc.layers[0];
  }

  /**
   * The layer a new shape goes on, and the command that adds it when the
   * document has no layers yet.
   */
  forNewShape(doc: DesignDocument): { layer: CutLayer; add: Command | null } {
    const layer = this.current(doc);
    if (layer) return { layer, add: null };
    const created = layerForSlot(doc, 0);
    return { layer: created, add: addLayer(created) };
  }
}

export const drawingLayer = new DrawingLayer();
