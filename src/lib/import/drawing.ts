import type { ImportedDrawing } from "#lib/bindings/ImportedDrawing.js";
import { addLayer, batch, insert, type Command } from "#lib/document/commands.js";
import {
  newId,
  type DesignDocument,
  type PathObject,
  type SceneObject,
} from "#lib/document/model.js";
import { IDENTITY } from "#lib/document/transform.js";
import { layersForColors } from "./layers.js";

export interface ImportPlan {
  /** One undoable step adding everything the drawing brings in, new layers included. */
  command: Command;
  /** The top-level object added: a group, or the path when there is only one. */
  object: SceneObject;
  /** Distinct layers the paths landed on. */
  layers: string[];
}

/**
 * Turns an imported drawing into document objects, each path on the layer
 * its colour maps to. The drawing keeps its page position: the page's
 * bottom-left corner sits on the bed origin.
 */
export function planImport(
  drawing: ImportedDrawing,
  name: string,
  doc: DesignDocument,
  makeId: () => string = newId,
): ImportPlan {
  const { layers, added } = layersForColors(
    drawing.paths.map((path) => path.color),
    doc,
  );
  const paths = drawing.paths.map((path, index): PathObject => ({
    type: "path",
    id: makeId(),
    name: path.name || `Path ${index + 1}`,
    layerId: layers.get(path.color)!,
    transform: IDENTITY,
    subpaths: path.subpaths,
  }));
  const title = name || `Imported ${drawing.format.toUpperCase()}`;
  const object: SceneObject =
    paths.length === 1
      ? { ...paths[0], name: paths[0].name === "Path 1" ? title : paths[0].name }
      : {
          type: "group",
          id: makeId(),
          name: title,
          layerId: paths[0].layerId,
          transform: IDENTITY,
          children: paths,
        };
  return {
    command:
      added.length === 0
        ? insert([object])
        : batch([...added.map((layer) => addLayer(layer)), insert([object])]),
    object,
    layers: [...new Set(paths.map((path) => path.layerId))],
  };
}

/** Whether pasted text is SVG markup. */
export function looksLikeSvg(text: string): boolean {
  return text.trimStart().startsWith("<") && /<svg[\s>]/i.test(text.slice(0, 8192));
}

/** A one-line summary for the user. */
export function describeImport(drawing: ImportedDrawing, name: string, layers: number): string {
  const size = `${round(drawing.width)} × ${round(drawing.height)} mm`;
  const paths = `${drawing.paths.length} ${drawing.paths.length === 1 ? "path" : "paths"}`;
  const onLayers = `${layers} ${layers === 1 ? "layer" : "layers"}`;
  const dpi = drawing.dpi === null ? "" : ` at ${drawing.dpi} DPI (${drawing.dpiReason})`;
  const units = drawing.units === null ? "" : `, drawn in ${drawing.units}`;
  const n = drawing.skipped;
  const skipped =
    n === 0
      ? ""
      : drawing.format === "svg"
        ? ` Left out ${n} ${n === 1 ? "image" : "images"}.`
        : ` Left out ${n} ${n === 1 ? "item" : "items"} that can’t be cut, such as text or hatching.`;
  const what = name ? `“${name}”` : drawing.format.toUpperCase();
  return `Imported ${what}: ${paths} on ${onLayers}, ${size}${dpi}${units}.${skipped}`;
}

function round(mm: number): string {
  return String(Math.round(mm * 100) / 100);
}

/** A unitless drawing's size read in `units`, in mm, e.g. "101.6 × 50.8 mm". */
export function sizeInUnits(size: { width: number; height: number }, units: "mm" | "in"): string {
  const scale = units === "in" ? 25.4 : 1;
  return `${round(size.width * scale)} × ${round(size.height * scale)} mm`;
}
