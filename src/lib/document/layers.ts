/**
 * Cut layers: the fixed colour palette, default settings and the checks a
 * layer must pass. Mirrors `CutLayer::validate` in `crates/bf-core`.
 */
import type { CutLayer, DesignDocument, SceneObject } from "./model.js";

/**
 * The 30 layer colours. The first 16 are the common laser-software palette from layer 01 on (00 is
 * black, which vanishes on the dark canvas); the rest are picked to stay
 * distinct and visible on both themes.
 */
export const LAYER_PALETTE: readonly string[] = [
  "#0000ff",
  "#ff0000",
  "#00e000",
  "#d0d000",
  "#ff8000",
  "#00e0e0",
  "#ff00ff",
  "#b4b4b4",
  "#0000a0",
  "#a00000",
  "#00a000",
  "#a0a000",
  "#c08000",
  "#00a0ff",
  "#a000a0",
  "#808080",
  "#8000ff",
  "#ff0080",
  "#80ff00",
  "#00ff80",
  "#ff80c0",
  "#80c0ff",
  "#c0a080",
  "#804000",
  "#408080",
  "#800040",
  "#ffc000",
  "#6060c0",
  "#c06060",
  "#004080",
];

/** Everything about a layer but its id: what `editLayer` changes. */
export type LayerChanges = Partial<Omit<CutLayer, "id">>;

/** A layer with default settings for palette entry `slot`. */
export function newLayer(slot: number, id = `layer-${slot + 1}`): CutLayer {
  return {
    id,
    name: `Layer ${slot + 1}`,
    color: LAYER_PALETTE[slot],
    mode: "line",
    speed: 1000,
    maxPower: 50,
    minPower: 0,
    passes: 1,
    constantPower: false,
    airAssist: true,
    output: true,
    shown: true,
    lineInterval: 0.1,
    scanAngle: 0,
    bidirectional: true,
    overscan: 2.5,
    fillGap: 1,
  };
}

/** A new layer for palette entry `slot`, with an id `doc` doesn't use yet. */
export function layerForSlot(doc: DesignDocument, slot: number): CutLayer {
  const taken = new Set(doc.layers.map((layer) => layer.id));
  let id = `layer-${slot + 1}`;
  for (let n = 2; taken.has(id); n++) id = `layer-${slot + 1}-${n}`;
  return newLayer(slot, id);
}

/** The first layer (in cut order) drawn in `color`. */
export function layerWithColor(doc: DesignDocument, color: string): CutLayer | undefined {
  return doc.layers.find((layer) => layer.color === color);
}

/** Why `layer` can't be cut, or null if its settings are fine. */
export function layerProblem(layer: CutLayer): string | null {
  const finite = (value: number) => Number.isFinite(value);
  if (!layer.id) return "A layer needs an id";
  if (!/^#[0-9a-f]{6}$/i.test(layer.color)) return "Layer colours are #rrggbb";
  if (!finite(layer.speed) || layer.speed <= 0) return "Speed must be above zero";
  const percent = (value: number) => finite(value) && value >= 0 && value <= 100;
  if (!percent(layer.maxPower) || !percent(layer.minPower)) return "Power is 0–100%";
  if (layer.minPower > layer.maxPower) return "Minimum power can't exceed maximum";
  if (!Number.isInteger(layer.passes) || layer.passes < 1) return "Passes is a whole number from 1";
  if (!finite(layer.lineInterval) || layer.lineInterval <= 0) {
    return "Line interval must be above zero";
  }
  if (!finite(layer.scanAngle)) return "Scan angle must be a number";
  if (!finite(layer.overscan) || layer.overscan < 0) return "Overscan can't be negative";
  if (!finite(layer.fillGap) || layer.fillGap < 0) return "Fill gap can't be negative";
  return null;
}

/** How many objects (groups and their contents counted separately) sit on each layer. */
export function layerUsage(objects: readonly SceneObject[]): Map<string, number> {
  const usage = new Map<string, number>();
  const visit = (list: readonly SceneObject[]) => {
    for (const object of list) {
      usage.set(object.layerId, (usage.get(object.layerId) ?? 0) + 1);
      if (object.type === "group") visit(object.children);
    }
  };
  visit(objects);
  return usage;
}

/** Millimetres between scan lines for a lines-per-inch figure, and back. */
export function lpiToInterval(lpi: number): number {
  return 25.4 / lpi;
}

export const intervalToLpi = lpiToInterval;
