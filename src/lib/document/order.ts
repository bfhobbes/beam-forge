/**
 * Z-order of top-level objects: bring forward and send backward one step, or
 * all the way to the front or back. Later objects draw over earlier ones.
 */
import { batch, remove, type Command } from "./commands.js";
import type { DesignDocument, SceneObject } from "./model.js";

export type OrderMove = "forward" | "backward" | "front" | "back";

/** The top-level order after moving `ids`: selected objects keep their relative order. */
export function reorder(
  objects: readonly SceneObject[],
  ids: readonly string[],
  move: OrderMove,
): SceneObject[] {
  const selected = new Set(ids);
  const picked = objects.filter((object) => selected.has(object.id));
  const others = objects.filter((object) => !selected.has(object.id));
  switch (move) {
    case "front":
      return [...others, ...picked];
    case "back":
      return [...picked, ...others];
    case "forward":
      return stepUp(objects, selected);
    case "backward":
      return stepUp(objects.toReversed(), selected).toReversed();
  }
}

/** Moves each selected object above the next unselected one, as a block. */
function stepUp(objects: readonly SceneObject[], selected: Set<string>): SceneObject[] {
  const result = [...objects];
  // Walk from the top down so a run of selected objects moves together.
  for (let i = result.length - 2; i >= 0; i--) {
    if (!selected.has(result[i].id)) continue;
    let end = i;
    while (end + 1 < result.length && selected.has(result[end + 1].id)) end++;
    if (end + 1 >= result.length) continue;
    const [over] = result.splice(end + 1, 1);
    result.splice(i, 0, over);
  }
  return result;
}

/**
 * The command that moves `ids` (top-level objects) in the stacking order, or
 * null when nothing would move. It removes and re-inserts the moved objects,
 * so undo puts them back.
 */
export function orderCommand(
  doc: DesignDocument,
  ids: readonly string[],
  move: OrderMove,
): Command | null {
  const next = reorder(doc.objects, ids, move);
  if (next.every((object, i) => object === doc.objects[i])) return null;
  const selected = new Set(ids);
  const moved = next.flatMap((object, index) =>
    selected.has(object.id) ? [{ object, index }] : [],
  );
  return batch([
    remove(moved.map(({ object }) => object.id)),
    // Ascending indexes: each insert lands where `next` has it.
    {
      type: "insert",
      items: moved.map(({ object, index }) => ({ parent: null, index, object })),
    },
  ]);
}
