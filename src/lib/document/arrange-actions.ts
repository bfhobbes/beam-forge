import type { Action } from "#lib/actions/registry.svelte.js";
import {
  alignObjects,
  distributeObjects,
  mirrorObjects,
  selectionBounds,
  type AlignEdge,
} from "#lib/canvas/arrange.js";
import { bedBounds, type Bed } from "#lib/canvas/bed.js";
import { group, ungroup } from "./commands.js";
import type { DocumentEditor } from "./editor.svelte.js";
import { newId, type GroupObject } from "./model.js";
import { orderCommand, type OrderMove } from "./order.js";
import type { Selection } from "./selection.svelte.js";

const edges: { edge: AlignEdge; label: string }[] = [
  { edge: "left", label: "left edges" },
  { edge: "centre", label: "centres horizontally" },
  { edge: "right", label: "right edges" },
  { edge: "top", label: "top edges" },
  { edge: "middle", label: "middles vertically" },
  { edge: "bottom", label: "bottom edges" },
];

const orderMoves: { id: string; move: OrderMove; title: string; keywords: string[] }[] = [
  { id: "bring-forward", move: "forward", title: "Bring forward", keywords: ["raise", "up"] },
  { id: "send-backward", move: "backward", title: "Send backward", keywords: ["lower", "down"] },
  { id: "bring-to-front", move: "front", title: "Bring to front", keywords: ["raise", "top"] },
  { id: "send-to-back", move: "back", title: "Send to back", keywords: ["lower", "bottom"] },
];

/**
 * Arrange menu actions over the selection: group and ungroup, align to the
 * selection or the bed, distribute, stacking order and mirror. Each run is
 * one undo step. `bed` is the work area the canvas shows.
 */
export function arrangeActions(
  editor: DocumentEditor,
  selection: Selection,
  bed: () => Bed,
): Action[] {
  const count = () => selection.ids.length;
  const groups = () =>
    editor.doc.objects.filter(
      (object): object is GroupObject => selection.has(object.id) && object.type === "group",
    );

  return [
    {
      id: "edit.group",
      title: "Group",
      group: "Arrange",
      keywords: ["combine"],
      enabled: () => count() >= 2,
      hint: "Select two or more objects",
      run: () => {
        const id = newId();
        if (editor.execute(group(selection.ids, id))) selection.set([id]);
      },
    },
    {
      id: "edit.ungroup",
      title: "Ungroup",
      group: "Arrange",
      keywords: ["split", "break apart"],
      enabled: () => groups().length > 0,
      hint: "Select a group",
      run: () => {
        const dissolved = groups();
        const others = selection.ids.filter((id) => !dissolved.some((g) => g.id === id));
        const members = dissolved.flatMap((g) => g.children.map((child) => child.id));
        if (editor.execute(ungroup(dissolved.map((g) => g.id)))) {
          selection.set([...others, ...members]);
        }
      },
    },
    ...edges.map(({ edge, label }) => ({
      id: `arrange.align-${edge}`,
      title: `Align ${label}`,
      group: "Arrange",
      keywords: ["align", "line up", edge],
      enabled: () => count() >= 2,
      hint: "Select two or more objects",
      run: () => {
        const bounds = selectionBounds(editor.doc, selection.ids);
        if (bounds) editor.execute(alignObjects(editor.doc, selection.ids, edge, bounds));
      },
    })),
    ...edges.map(({ edge, label }) => ({
      id: `arrange.align-${edge}-bed`,
      title: `Align ${label} to the bed`,
      group: "Arrange",
      keywords: ["align", "work area", "page", edge],
      enabled: () => count() >= 1,
      hint: "Select something first",
      run: () =>
        void editor.execute(alignObjects(editor.doc, selection.ids, edge, bedBounds(bed()))),
    })),
    {
      id: "arrange.distribute-horizontal",
      title: "Distribute horizontal spacing",
      group: "Arrange",
      keywords: ["distribute", "space evenly", "gaps"],
      enabled: () => count() >= 3,
      hint: "Select three or more objects",
      run: () => void editor.execute(distributeObjects(editor.doc, selection.ids, "x")),
    },
    {
      id: "arrange.distribute-vertical",
      title: "Distribute vertical spacing",
      group: "Arrange",
      keywords: ["distribute", "space evenly", "gaps"],
      enabled: () => count() >= 3,
      hint: "Select three or more objects",
      run: () => void editor.execute(distributeObjects(editor.doc, selection.ids, "y")),
    },
    ...orderMoves.map(({ id, move, title, keywords }) => ({
      id: `arrange.${id}`,
      title,
      group: "Arrange",
      keywords: ["order", "z-order", "stack", ...keywords],
      enabled: () => count() >= 1 && orderCommand(editor.doc, selection.ids, move) !== null,
      hint: "Select something that is not already there",
      run: () => {
        const command = orderCommand(editor.doc, selection.ids, move);
        if (command) editor.execute(command);
      },
    })),
    {
      id: "arrange.mirror-horizontal",
      title: "Mirror horizontally",
      group: "Arrange",
      keywords: ["flip", "reflect", "left right"],
      enabled: () => count() >= 1,
      hint: "Select something first",
      run: () => {
        const command = mirrorObjects(editor.doc, selection.ids, "horizontal");
        if (command) editor.execute(command);
      },
    },
    {
      id: "arrange.mirror-vertical",
      title: "Mirror vertically",
      group: "Arrange",
      keywords: ["flip", "reflect", "upside down"],
      enabled: () => count() >= 1,
      hint: "Select something first",
      run: () => {
        const command = mirrorObjects(editor.doc, selection.ids, "vertical");
        if (command) editor.execute(command);
      },
    },
  ];
}
