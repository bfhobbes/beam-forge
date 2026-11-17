/**
 * Commands are plain data describing one edit. `applyCommand` is pure: it
 * returns the new document and the command that exactly undoes the edit.
 */
import { layerProblem, layerUsage, type LayerChanges } from "./layers.js";
import {
  allIds,
  findPlacement,
  type CutLayer,
  type DesignDocument,
  type GroupObject,
  type Placement,
  type SceneObject,
  type TextObject,
} from "./model.js";
import { hasShape, SHAPE_FIELDS, shapeProblem, toPath, type ShapeChanges } from "./shapes.js";
import { IDENTITY, multiply, type Transform } from "./transform.js";

export interface InsertItem {
  /** Group to insert into; null for the top level. */
  parent: string | null;
  /** Position in the parent's children; null appends. */
  index: number | null;
  object: SceneObject;
}

export type Command =
  | { type: "insert"; items: InsertItem[] }
  | { type: "remove"; ids: string[] }
  | { type: "setTransforms"; transforms: Record<string, Transform> }
  | { type: "group"; ids: string[]; groupId: string; name: string }
  | { type: "ungroup"; ids: string[] }
  | { type: "addLayer"; layer: CutLayer; index: number | null }
  | { type: "removeLayer"; id: string }
  | { type: "moveLayer"; id: string; index: number }
  | { type: "editLayer"; id: string; changes: LayerChanges }
  | { type: "setObjectLayers"; layers: Record<string, string> }
  | { type: "editShape"; id: string; changes: ShapeChanges }
  | { type: "editText"; id: string; changes: TextChanges }
  | { type: "batch"; commands: Command[] };

/** What a text edit may change: its string, style and the outline they make. */
export type TextChanges = Partial<
  Pick<
    TextObject,
    | "text"
    | "fontFamily"
    | "fontWeight"
    | "italic"
    | "height"
    | "letterSpacing"
    | "lineSpacing"
    | "align"
    | "outline"
  >
>;

/** Why `text`'s style can't be used, or null if it is fine. */
export function textProblem(text: TextObject): string | null {
  if (!Number.isFinite(text.height) || text.height <= 0) return "Height must be above zero";
  if (!Number.isFinite(text.lineSpacing) || text.lineSpacing <= 0) {
    return "Line spacing must be above zero";
  }
  if (!Number.isFinite(text.letterSpacing)) return "Letter spacing must be a number";
  if (!Number.isInteger(text.fontWeight) || text.fontWeight < 1 || text.fontWeight > 1000) {
    return "Weight is a whole number from 1 to 1000";
  }
  return null;
}

export interface Applied {
  doc: DesignDocument;
  inverse: Command;
}

/** A command that does not fit the document, e.g. an unknown or duplicate id. */
export class DocumentError extends Error {
  override name = "DocumentError";
}

/** Inserts objects side by side, appended or starting at `index`. */
export function insert(
  objects: SceneObject[],
  { parent = null, index = null }: { parent?: string | null; index?: number | null } = {},
): Command {
  return {
    type: "insert",
    items: objects.map((object, i) => ({
      parent,
      index: index === null ? null : index + i,
      object,
    })),
  };
}

export function remove(ids: string[]): Command {
  return { type: "remove", ids };
}

/** Sets absolute transforms by object id. */
export function setTransforms(transforms: Record<string, Transform>): Command {
  return { type: "setTransforms", transforms };
}

/** Wraps siblings in a new group at the topmost member's position. */
export function group(ids: string[], groupId: string, name = "Group"): Command {
  return { type: "group", ids, groupId, name };
}

/** Dissolves groups in place, baking each group's transform into its children. */
export function ungroup(ids: string[]): Command {
  return { type: "ungroup", ids };
}

/** Adds a cut layer, appended (cut last) or at `index` in cut order. */
export function addLayer(layer: CutLayer, index: number | null = null): Command {
  return { type: "addLayer", layer, index };
}

/** Removes a layer no object is on. */
export function removeLayer(id: string): Command {
  return { type: "removeLayer", id };
}

/** Moves a layer to `index` in cut order. */
export function moveLayer(id: string, index: number): Command {
  return { type: "moveLayer", id, index };
}

/** Changes a layer's name, colour or settings. */
export function editLayer(id: string, changes: LayerChanges): Command {
  return { type: "editLayer", id, changes };
}

/** Sets objects' layers by id, exactly: a group's children are not touched. */
export function setObjectLayers(layers: Record<string, string>): Command {
  return { type: "setObjectLayers", layers };
}

/** Puts objects on a layer: groups and everything in them. */
export function assignLayer(doc: DesignDocument, ids: readonly string[], layerId: string): Command {
  const layers: Record<string, string> = {};
  for (const id of ids) {
    const placement = findPlacement(doc.objects, id);
    if (!placement) throw new DocumentError(`No object with id ${id}`);
    for (const member of allIds([placement.object])) layers[member] = layerId;
  }
  return setObjectLayers(layers);
}

/** Changes a rectangle's, ellipse's or polygon's size or radii. */
export function editShape(id: string, changes: ShapeChanges): Command {
  return { type: "editShape", id, changes };
}

/**
 * Changes a text object's string or style. Pass the outline laid out for the
 * result with them, so the document never holds text and an outline that
 * disagree.
 */
export function editText(id: string, changes: TextChanges): Command {
  return { type: "editText", id, changes };
}

/**
 * Replaces the shapes in the given objects (groups included) with paths of
 * the same outline, in place and keeping their ids. Objects without shapes
 * are left alone.
 */
export function convertToPath(doc: DesignDocument, ids: readonly string[]): Command {
  return batch(
    ids.flatMap((id) => {
      const { parent, index, object } = placementOf(doc, id);
      if (!hasShape(object)) return [];
      return [
        batch([
          remove([id]),
          { type: "insert", items: [{ parent, index, object: toPath(object) }] },
        ]),
      ];
    }),
  );
}

export function batch(commands: Command[]): Command {
  return { type: "batch", commands };
}

export function applyCommand(doc: DesignDocument, command: Command): Applied {
  switch (command.type) {
    case "insert":
      return applyInsert(doc, command.items);
    case "remove":
      return applyRemove(doc, command.ids);
    case "setTransforms":
      return applySetTransforms(doc, command.transforms);
    case "group":
      return applyGroup(doc, command);
    case "ungroup":
      return sequence(
        doc,
        command.ids.map((id) => (current) => applyUngroupOne(current, id)),
      );
    case "addLayer":
      return applyAddLayer(doc, command.layer, command.index);
    case "removeLayer":
      return applyRemoveLayer(doc, command.id);
    case "moveLayer":
      return applyMoveLayer(doc, command.id, command.index);
    case "editLayer":
      return applyEditLayer(doc, command.id, command.changes);
    case "setObjectLayers":
      return applySetObjectLayers(doc, command.layers);
    case "editShape":
      return applyEditShape(doc, command.id, command.changes);
    case "editText":
      return applyEditText(doc, command.id, command.changes);
    case "batch":
      return sequence(
        doc,
        command.commands.map((step) => (current) => applyCommand(current, step)),
      );
  }
}

/**
 * Merges a command into the one before it when the pair can be one command,
 * e.g. successive transform updates during a drag. Returns null otherwise.
 */
export function merge(first: Command, second: Command): Command | null {
  if (first.type === "setTransforms" && second.type === "setTransforms") {
    return setTransforms({ ...first.transforms, ...second.transforms });
  }
  if (first.type === "editLayer" && second.type === "editLayer" && first.id === second.id) {
    return editLayer(first.id, { ...first.changes, ...second.changes });
  }
  if (first.type === "editShape" && second.type === "editShape" && first.id === second.id) {
    return editShape(first.id, { ...first.changes, ...second.changes });
  }
  if (first.type === "editText" && second.type === "editText" && first.id === second.id) {
    return editText(first.id, { ...first.changes, ...second.changes });
  }
  return null;
}

/** One command that applies `first` then `second`, merging where they meet. */
export function compose(first: Command, second: Command): Command {
  const a = first.type === "batch" ? first.commands : [first];
  const b = second.type === "batch" ? second.commands : [second];
  const joined = a.length > 0 && b.length > 0 ? merge(a[a.length - 1], b[0]) : null;
  const commands = joined ? [...a.slice(0, -1), joined, ...b.slice(1)] : [...a, ...b];
  return commands.length === 1 ? commands[0] : batch(commands);
}

function applyInsert(doc: DesignDocument, items: InsertItem[]): Applied {
  const existing = new Set(allIds(doc.objects));
  let objects = doc.objects;
  for (const { parent, index, object } of items) {
    for (const id of allIds([object])) {
      if (existing.has(id)) throw new DocumentError(`Duplicate object id ${id}`);
      existing.add(id);
    }
    objects = editChildren(objects, parent, (children) => {
      if (index !== null && (index < 0 || index > children.length)) {
        throw new DocumentError(`Insert index ${index} is out of range`);
      }
      return children.toSpliced(index ?? children.length, 0, object);
    });
  }
  return { doc: { ...doc, objects }, inverse: remove(items.map((item) => item.object.id)) };
}

function applyRemove(doc: DesignDocument, ids: string[]): Applied {
  const placements = ids.map((id) => placementOf(doc, id));
  const removed = new Set(ids);
  // A descendant of another removed object goes with its ancestor.
  const roots = placements.filter((placement) => !hasRemovedAncestor(doc, placement, removed));
  let objects = doc.objects;
  for (const { parent, object } of roots) {
    objects = editChildren(objects, parent, (children) =>
      children.filter((child) => child.id !== object.id),
    );
  }
  // Re-inserting in ascending index order puts each object back where it was.
  const restore = roots
    .toSorted((a, b) => a.index - b.index)
    .map(({ parent, index, object }) => ({ parent, index, object }));
  return { doc: { ...doc, objects }, inverse: { type: "insert", items: restore } };
}

function applySetTransforms(doc: DesignDocument, transforms: Record<string, Transform>): Applied {
  const previous: Record<string, Transform> = {};
  let objects = doc.objects;
  for (const [id, transform] of Object.entries(transforms)) {
    const { parent, object } = placementOf(doc, id);
    previous[id] = object.transform;
    objects = editChildren(objects, parent, (children) =>
      children.map((child) => (child.id === id ? { ...child, transform } : child)),
    );
  }
  return { doc: { ...doc, objects }, inverse: setTransforms(previous) };
}

function applyGroup(
  doc: DesignDocument,
  { ids, groupId, name }: { ids: string[]; groupId: string; name: string },
): Applied {
  if (ids.length === 0) throw new DocumentError("Nothing to group");
  if (findPlacement(doc.objects, groupId))
    throw new DocumentError(`Duplicate object id ${groupId}`);
  const members = ids.map((id) => placementOf(doc, id)).toSorted((a, b) => a.index - b.index);
  const parent = members[0].parent;
  if (members.some((member) => member.parent !== parent)) {
    throw new DocumentError("Only objects with the same parent can be grouped");
  }
  const created: GroupObject = {
    type: "group",
    id: groupId,
    name,
    layerId: members[0].object.layerId,
    transform: IDENTITY,
    children: members.map((member) => member.object),
  };
  const memberIds = new Set(ids);
  const topmost = members[members.length - 1].index - (members.length - 1);
  const objects = editChildren(doc.objects, parent, (children) =>
    children.filter((child) => !memberIds.has(child.id)).toSpliced(topmost, 0, created),
  );
  const restore = members.map(({ index, object }) => ({ parent, index, object }));
  return {
    doc: { ...doc, objects },
    inverse: batch([remove([groupId]), { type: "insert", items: restore }]),
  };
}

function applyUngroupOne(doc: DesignDocument, id: string): Applied {
  const { parent, index, object } = placementOf(doc, id);
  if (object.type !== "group") throw new DocumentError(`${id} is not a group`);
  const children = object.children.map((child) => ({
    ...child,
    transform: multiply(object.transform, child.transform),
  }));
  const objects = editChildren(doc.objects, parent, (siblings) =>
    siblings.toSpliced(index, 1, ...children),
  );
  return {
    doc: { ...doc, objects },
    inverse: batch([
      remove(children.map((child) => child.id)),
      { type: "insert", items: [{ parent, index, object }] },
    ]),
  };
}

function layerIndex(doc: DesignDocument, id: string): number {
  const index = doc.layers.findIndex((layer) => layer.id === id);
  if (index === -1) throw new DocumentError(`No layer with id ${id}`);
  return index;
}

function applyAddLayer(doc: DesignDocument, layer: CutLayer, index: number | null): Applied {
  if (doc.layers.some((other) => other.id === layer.id)) {
    throw new DocumentError(`Duplicate layer id ${layer.id}`);
  }
  const problem = layerProblem(layer);
  if (problem) throw new DocumentError(problem);
  if (index !== null && (index < 0 || index > doc.layers.length)) {
    throw new DocumentError(`Layer index ${index} is out of range`);
  }
  const layers = doc.layers.toSpliced(index ?? doc.layers.length, 0, layer);
  return { doc: { ...doc, layers }, inverse: removeLayer(layer.id) };
}

function applyRemoveLayer(doc: DesignDocument, id: string): Applied {
  const index = layerIndex(doc, id);
  if (layerUsage(doc.objects).has(id)) {
    throw new DocumentError(`Layer ${doc.layers[index].name} still has objects on it`);
  }
  return {
    doc: { ...doc, layers: doc.layers.toSpliced(index, 1) },
    inverse: addLayer(doc.layers[index], index),
  };
}

function applyMoveLayer(doc: DesignDocument, id: string, index: number): Applied {
  const from = layerIndex(doc, id);
  if (!Number.isInteger(index) || index < 0 || index >= doc.layers.length) {
    throw new DocumentError(`Layer index ${index} is out of range`);
  }
  const layers = doc.layers.toSpliced(from, 1).toSpliced(index, 0, doc.layers[from]);
  return { doc: { ...doc, layers }, inverse: moveLayer(id, from) };
}

function applyEditLayer(doc: DesignDocument, id: string, changes: LayerChanges): Applied {
  const index = layerIndex(doc, id);
  const layer = doc.layers[index];
  const edited = { ...layer, ...changes, id };
  const problem = layerProblem(edited);
  if (problem) throw new DocumentError(problem);
  const previous = Object.fromEntries(
    Object.keys(changes).map((key) => [key, layer[key as keyof LayerChanges]]),
  ) as LayerChanges;
  return {
    doc: { ...doc, layers: doc.layers.with(index, edited) },
    inverse: editLayer(id, previous),
  };
}

function applySetObjectLayers(doc: DesignDocument, layers: Record<string, string>): Applied {
  const known = new Set(doc.layers.map((layer) => layer.id));
  const previous: Record<string, string> = {};
  let objects = doc.objects;
  for (const [id, layerId] of Object.entries(layers)) {
    if (!known.has(layerId)) throw new DocumentError(`No layer with id ${layerId}`);
    const { parent, object } = placementOf(doc, id);
    previous[id] = object.layerId;
    objects = editChildren(objects, parent, (children) =>
      children.map((child) => (child.id === id ? { ...child, layerId } : child)),
    );
  }
  return { doc: { ...doc, objects }, inverse: setObjectLayers(previous) };
}

function applyEditShape(doc: DesignDocument, id: string, changes: ShapeChanges): Applied {
  const { parent, object } = placementOf(doc, id);
  if (object.type !== "rect" && object.type !== "ellipse" && object.type !== "polygon") {
    throw new DocumentError(`${object.name} is not a rectangle, ellipse or polygon`);
  }
  const fields: readonly string[] = SHAPE_FIELDS[object.type];
  const keys = Object.keys(changes) as (keyof ShapeChanges)[];
  const wrong = keys.find((key) => !fields.includes(key));
  if (wrong) throw new DocumentError(`A ${object.type} has no ${wrong}`);
  const edited = { ...object, ...changes } as typeof object;
  const problem = shapeProblem(edited);
  if (problem) throw new DocumentError(problem);
  const previous = Object.fromEntries(
    keys.map((key) => [key, (object as unknown as Record<string, unknown>)[key]]),
  ) as ShapeChanges;
  const objects = editChildren(doc.objects, parent, (children) =>
    children.map((child) => (child.id === id ? edited : child)),
  );
  return { doc: { ...doc, objects }, inverse: editShape(id, previous) };
}

function applyEditText(doc: DesignDocument, id: string, changes: TextChanges): Applied {
  const { parent, object } = placementOf(doc, id);
  if (object.type !== "text") throw new DocumentError(`${object.name} is not text`);
  const edited: TextObject = { ...object, ...changes };
  const problem = textProblem(edited);
  if (problem) throw new DocumentError(problem);
  const keys = Object.keys(changes) as (keyof TextChanges)[];
  const previous = Object.fromEntries(keys.map((key) => [key, object[key]])) as TextChanges;
  const objects = editChildren(doc.objects, parent, (children) =>
    children.map((child) => (child.id === id ? edited : child)),
  );
  return { doc: { ...doc, objects }, inverse: editText(id, previous) };
}

/** Applies steps in order; the inverse undoes them in reverse. */
function sequence(doc: DesignDocument, steps: ((doc: DesignDocument) => Applied)[]): Applied {
  const inverses: Command[] = [];
  for (const step of steps) {
    const applied = step(doc);
    doc = applied.doc;
    inverses.push(applied.inverse);
  }
  return { doc, inverse: batch(inverses.toReversed()) };
}

function placementOf(doc: DesignDocument, id: string): Placement {
  const placement = findPlacement(doc.objects, id);
  if (!placement) throw new DocumentError(`No object with id ${id}`);
  return placement;
}

function hasRemovedAncestor(
  doc: DesignDocument,
  { parent }: Placement,
  removed: Set<string>,
): boolean {
  while (parent !== null) {
    if (removed.has(parent)) return true;
    parent = placementOf(doc, parent).parent;
  }
  return false;
}

/** Returns a new tree with the children of `parent` (null: top level) replaced. */
function editChildren(
  objects: SceneObject[],
  parent: string | null,
  edit: (children: SceneObject[]) => SceneObject[],
): SceneObject[] {
  if (parent === null) return edit(objects);
  let found = false;
  const visit = (list: SceneObject[]): SceneObject[] =>
    list.map((object) => {
      if (found || object.type !== "group") return object;
      if (object.id === parent) {
        found = true;
        return { ...object, children: edit(object.children) };
      }
      const children = visit(object.children);
      return children.some((child, i) => child !== object.children[i])
        ? { ...object, children }
        : object;
    });
  const result = visit(objects);
  if (!found) throw new DocumentError(`No group with id ${parent}`);
  return result;
}
