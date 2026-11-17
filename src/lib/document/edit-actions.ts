import type { Action } from "#lib/actions/registry.svelte.js";
import { convertToPath, remove, setTransforms } from "./commands.js";
import type { DocumentEditor } from "./editor.svelte.js";
import type { Selection } from "./selection.svelte.js";
import { hasShape } from "./shapes.js";
import { multiply, translate } from "./transform.js";

/** Arrow-key nudge distances in mm. */
export const NUDGE = 1;
export const NUDGE_FAR = 10;

const directions = [
  { name: "left", label: "Left", dx: -1, dy: 0 },
  { name: "right", label: "Right", dx: 1, dy: 0 },
  // Y is up, matching the machine.
  { name: "up", label: "Up", dx: 0, dy: 1 },
  { name: "down", label: "Down", dx: 0, dy: -1 },
] as const;

/**
 * Edit menu actions over the open document and its selection: undo, redo,
 * delete, select all, clear, arrow-key nudges and convert to path. Each run
 * is one undo step.
 */
export function editActions(editor: DocumentEditor, selection: Selection): Action[] {
  const hasSelection = () => selection.ids.length > 0;
  const nudge = (dx: number, dy: number) => {
    const by = translate(dx, dy);
    const transforms = Object.fromEntries(
      editor.doc.objects
        .filter((object) => selection.has(object.id))
        .map((object) => [object.id, multiply(by, object.transform)]),
    );
    editor.execute(setTransforms(transforms));
  };

  return [
    {
      id: "edit.undo",
      title: "Undo",
      group: "Edit",
      enabled: () => editor.canUndo,
      hint: "Nothing to undo",
      run: () => void editor.undo(),
    },
    {
      id: "edit.redo",
      title: "Redo",
      group: "Edit",
      enabled: () => editor.canRedo,
      hint: "Nothing to redo",
      run: () => void editor.redo(),
    },
    {
      id: "edit.delete",
      title: "Delete",
      group: "Edit",
      keywords: ["remove"],
      enabled: hasSelection,
      hint: "Select something first",
      run: () => {
        if (editor.execute(remove(selection.ids))) selection.clear();
      },
    },
    {
      id: "edit.select-all",
      title: "Select all",
      group: "Edit",
      enabled: () => editor.doc.objects.length > 0,
      hint: "The design is empty",
      run: () => selection.selectAll(),
    },
    {
      id: "edit.deselect",
      title: "Clear selection",
      group: "Edit",
      keywords: ["deselect", "select none"],
      enabled: hasSelection,
      run: () => selection.clear(),
    },
    {
      id: "edit.convert-to-path",
      title: "Convert to path",
      group: "Edit",
      keywords: ["outline", "curves", "path", "rectangle", "ellipse", "polygon"],
      enabled: () =>
        editor.doc.objects.some((object) => selection.has(object.id) && hasShape(object)),
      hint: "Select a rectangle, ellipse or polygon",
      run: () => void editor.execute(convertToPath(editor.doc, selection.ids)),
    },
    ...directions.flatMap(({ name, label, dx, dy }) => [
      {
        id: `edit.nudge-${name}`,
        title: `Nudge ${label.toLowerCase()} ${NUDGE} mm`,
        group: "Edit",
        keywords: ["move"],
        enabled: hasSelection,
        hint: "Select something first",
        run: () => nudge(dx * NUDGE, dy * NUDGE),
      },
      {
        id: `edit.nudge-${name}-far`,
        title: `Nudge ${label.toLowerCase()} ${NUDGE_FAR} mm`,
        group: "Edit",
        keywords: ["move"],
        enabled: hasSelection,
        hint: "Select something first",
        run: () => nudge(dx * NUDGE_FAR, dy * NUDGE_FAR),
      },
    ]),
  ];
}
