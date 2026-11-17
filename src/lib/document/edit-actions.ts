import type { Action } from "#lib/actions/registry.svelte.js";
import { anchorPoint, CENTRE, selectionBounds } from "#lib/canvas/arrange.js";
import { clipboard as appClipboard, type TextClipboard } from "#lib/desktop/clipboard.svelte.js";
import {
  copyContent,
  duplicateCommand,
  parseContent,
  pasteCommand,
  serializeContent,
} from "./clipboard.js";
import { convertToPath, DocumentError, remove, setTransforms, type Command } from "./commands.js";
import type { DocumentEditor } from "./editor.svelte.js";
import type { Selection } from "./selection.svelte.js";
import { hasShape } from "./shapes.js";
import type { Point } from "./model.js";
import { multiply, translate } from "./transform.js";

/** Arrow-key nudge distances in mm. */
export const NUDGE = 1;
export const NUDGE_FAR = 10;
/** How far a duplicate lands from its original, in mm: right and down. */
export const DUPLICATE_OFFSET: Point = { x: 10, y: -10 };

export interface EditActionOptions {
  clipboard?: TextClipboard;
  /** Where on the bed "Paste" puts things: the pointer's last position over the canvas, in mm. */
  pasteTarget?: () => Point | null;
}

const directions = [
  { name: "left", label: "Left", dx: -1, dy: 0 },
  { name: "right", label: "Right", dx: 1, dy: 0 },
  // Y is up, matching the machine.
  { name: "up", label: "Up", dx: 0, dy: 1 },
  { name: "down", label: "Down", dx: 0, dy: -1 },
] as const;

/**
 * Edit menu actions over the open document and its selection: undo, redo,
 * cut, copy, paste, duplicate, delete, select all, clear, arrow-key nudges
 * and convert to path. Each run is one undo step.
 */
export function editActions(
  editor: DocumentEditor,
  selection: Selection,
  { clipboard = appClipboard, pasteTarget = () => null }: EditActionOptions = {},
): Action[] {
  const hasSelection = () => selection.ids.length > 0;
  /** Runs a command that adds objects and selects them. */
  const place = ({ command, ids }: { command: Command; ids: string[] }) => {
    try {
      if (editor.execute(command)) selection.set(ids);
    } catch (error) {
      // Pasted layers can fail the document's checks; the paste just doesn't happen.
      if (!(error instanceof DocumentError)) throw error;
      console.warn("Paste refused", error);
    }
  };
  const copy = () => clipboard.write(serializeContent(copyContent(editor.doc, selection.ids)));
  const paste = async (atPointer: boolean) => {
    const text = await clipboard.read();
    const content = text === null ? null : parseContent(text);
    if (!content) return;
    // At the pointer, the pasted objects are centred on it; otherwise they keep their place.
    let offset = { x: 0, y: 0 };
    const target = atPointer ? pasteTarget() : null;
    const pasted = { ...editor.doc, objects: content.objects };
    const bounds =
      target &&
      selectionBounds(
        pasted,
        content.objects.map((object) => object.id),
      );
    if (target && bounds) {
      const centre = anchorPoint(bounds, CENTRE);
      offset = { x: target.x - centre.x, y: target.y - centre.y };
    }
    place(pasteCommand(editor.doc, content, offset));
  };
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
      id: "edit.cut",
      title: "Cut",
      group: "Edit",
      keywords: ["clipboard"],
      enabled: hasSelection,
      hint: "Select something first",
      run: () => {
        const ids = selection.ids;
        // The copy is taken before the delete, so it holds what was selected.
        void copy();
        if (editor.execute(remove(ids))) selection.clear();
      },
    },
    {
      id: "edit.copy",
      title: "Copy",
      group: "Edit",
      keywords: ["clipboard"],
      enabled: hasSelection,
      hint: "Select something first",
      run: () => void copy(),
    },
    {
      id: "edit.paste",
      title: "Paste at pointer",
      group: "Edit",
      keywords: ["clipboard", "paste at cursor"],
      enabled: () => clipboard.mayHaveText,
      hint: "Copy something first",
      run: () => void paste(true),
    },
    {
      id: "edit.paste-in-place",
      title: "Paste in place",
      group: "Edit",
      keywords: ["clipboard", "same position"],
      enabled: () => clipboard.mayHaveText,
      hint: "Copy something first",
      run: () => void paste(false),
    },
    {
      id: "edit.duplicate",
      title: "Duplicate",
      group: "Edit",
      keywords: ["copy", "clone"],
      enabled: hasSelection,
      hint: "Select something first",
      run: () => place(duplicateCommand(editor.doc, selection.ids, DUPLICATE_OFFSET)),
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
