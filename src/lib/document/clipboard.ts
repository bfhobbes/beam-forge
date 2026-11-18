/**
 * Copying objects between documents: the clipboard text format and the
 * commands that paste or duplicate objects. Copies take their cut layers
 * with them, so pasting into another document cuts with the same settings.
 */
import { addLayer, batch, insert, type Command } from "./commands.js";
import {
  newId,
  type CutLayer,
  type DesignDocument,
  type Point,
  type SceneObject,
} from "./model.js";
import { multiply, translate } from "./transform.js";

/** Marks clipboard text as Beam Forge objects. */
export const CLIPBOARD_FORMAT = "beam-forge/objects";

/** What the clipboard holds: top-level objects, bottom to top, and the layers they use. */
export interface ClipboardContent {
  format: typeof CLIPBOARD_FORMAT;
  version: 1;
  objects: SceneObject[];
  layers: CutLayer[];
}

const OBJECT_TYPES = new Set(["path", "rect", "ellipse", "polygon", "text", "group"]);

function* layerIds(objects: readonly SceneObject[]): Generator<string> {
  for (const object of objects) {
    yield object.layerId;
    if (object.type === "group") yield* layerIds(object.children);
  }
}

/** The selected top-level objects in stacking order, with their layers. */
export function copyContent(doc: DesignDocument, ids: readonly string[]): ClipboardContent {
  const wanted = new Set(ids);
  const objects = doc.objects.filter((object) => wanted.has(object.id));
  const used = new Set(layerIds(objects));
  return {
    format: CLIPBOARD_FORMAT,
    version: 1,
    objects,
    layers: doc.layers.filter((layer) => used.has(layer.id)),
  };
}

export function serializeContent(content: ClipboardContent): string {
  return JSON.stringify(content);
}

const isRecord = (value: unknown): value is Record<string, unknown> =>
  typeof value === "object" && value !== null && !Array.isArray(value);

function looksLikeObject(value: unknown): boolean {
  if (!isRecord(value)) return false;
  if (typeof value.id !== "string" || typeof value.layerId !== "string") return false;
  if (typeof value.type !== "string" || !OBJECT_TYPES.has(value.type)) return false;
  if (!Array.isArray(value.transform) || value.transform.length !== 6) return false;
  return (
    value.type !== "group" ||
    (Array.isArray(value.children) && value.children.every(looksLikeObject))
  );
}

/** Reads clipboard text; null for anything that is not Beam Forge objects. */
export function parseContent(text: string): ClipboardContent | null {
  let value: unknown;
  try {
    value = JSON.parse(text);
  } catch {
    return null;
  }
  if (!isRecord(value) || value.format !== CLIPBOARD_FORMAT || value.version !== 1) return null;
  const { objects, layers } = value;
  if (!Array.isArray(objects) || objects.length === 0 || !objects.every(looksLikeObject)) {
    return null;
  }
  if (
    !Array.isArray(layers) ||
    !layers.every((layer) => isRecord(layer) && typeof layer.id === "string")
  ) {
    return null;
  }
  return value as unknown as ClipboardContent;
}

/** A deep copy of `object` with fresh ids, its group members included. */
export function cloneWithNewIds(object: SceneObject): SceneObject {
  const id = newId();
  if (object.type === "group") {
    return { ...object, id, children: object.children.map(cloneWithNewIds) };
  }
  return { ...structuredClone(object), id };
}

/** Commands that add the layers `objects` use but `doc` lacks, from `layers`. */
function missingLayers(
  doc: DesignDocument,
  objects: readonly SceneObject[],
  layers: readonly CutLayer[],
): Command[] {
  const known = new Set(doc.layers.map((layer) => layer.id));
  const needed = new Set(layerIds(objects));
  return layers
    .filter((layer) => needed.has(layer.id) && !known.has(layer.id))
    .map((layer) => addLayer(layer));
}

export interface Placed {
  command: Command;
  /** The new objects' ids, to select. */
  ids: string[];
}

/**
 * Pastes `content` on top of everything, moved by `offset` mm. Layers the
 * document lacks are added; a layer it already has (same id) is used as is.
 */
export function pasteCommand(
  doc: DesignDocument,
  content: ClipboardContent,
  offset: Point = { x: 0, y: 0 },
): Placed {
  const by = translate(offset.x, offset.y);
  const objects = content.objects.map((object) => {
    const copy = cloneWithNewIds(object);
    return { ...copy, transform: multiply(by, copy.transform) };
  });
  return {
    command: batch([...missingLayers(doc, objects, content.layers), insert(objects)]),
    ids: objects.map((object) => object.id),
  };
}

/** Copies the selected top-level objects on top of everything, moved by `offset` mm. */
export function duplicateCommand(
  doc: DesignDocument,
  ids: readonly string[],
  offset: Point,
): Placed {
  return pasteCommand(doc, copyContent(doc, ids), offset);
}
