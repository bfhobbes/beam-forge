/**
 * Colour-to-layer mapping for imported artwork: each colour
 * lands on the layer drawn in the nearest palette colour.
 */
import { layerForSlot, LAYER_PALETTE } from "#lib/document/layers.js";
import type { CutLayer, DesignDocument } from "#lib/document/model.js";

/** RGB distance within which an imported colour counts as a palette colour. */
const MATCH_DISTANCE = 48;

function rgb(hex: string): [number, number, number] {
  const value = Number.parseInt(hex.slice(1), 16);
  return [(value >> 16) & 0xff, (value >> 8) & 0xff, value & 0xff];
}

function distance(a: string, b: string): number {
  const [r1, g1, b1] = rgb(a);
  const [r2, g2, b2] = rgb(b);
  return Math.hypot(r1 - r2, g1 - g2, b1 - b2);
}

/** The palette entry nearest `color`, if it is within `limit`. */
function nearestSlot(color: string, limit = Infinity): number | null {
  let best: number | null = null;
  for (const [slot, entry] of LAYER_PALETTE.entries()) {
    const d = distance(color, entry);
    if (d <= limit && (best === null || d < distance(color, LAYER_PALETTE[best]))) best = slot;
  }
  return best;
}

export interface ColorLayers {
  /** The layer id for each colour. */
  layers: Map<string, string>;
  /** Layers the document doesn't have yet, for the import to add. */
  added: CutLayer[];
}

/**
 * Picks a layer for each `#rrggbb` colour. A colour at (or near) a palette
 * colour goes to the layer in that colour: an existing one if the document
 * has it, otherwise a new one. Any other colour, black included, gets a new
 * layer in the next palette colour nobody uses, so distinct colours stay on
 * distinct layers until the palette runs out.
 */
export function layersForColors(colors: Iterable<string>, doc: DesignDocument): ColorLayers {
  const layers = new Map<string, string>();
  const added: CutLayer[] = [];
  const all = () => [...doc.layers, ...added];
  const layerFor = (slot: number) => {
    const existing = all().find((layer) => layer.color === LAYER_PALETTE[slot]);
    if (existing) return existing.id;
    const layer = layerForSlot({ ...doc, layers: all() }, slot);
    added.push(layer);
    return layer.id;
  };
  const unmatched: string[] = [];
  for (const color of new Set(colors)) {
    const slot = nearestSlot(color, MATCH_DISTANCE);
    if (slot === null) unmatched.push(color);
    else layers.set(color, layerFor(slot));
  }
  for (const color of unmatched) {
    const used = new Set(all().map((layer) => layer.color));
    const free = LAYER_PALETTE.findIndex((entry) => !used.has(entry));
    layers.set(color, layerFor(free === -1 ? (nearestSlot(color) ?? 0) : free));
  }
  return { layers, added };
}
