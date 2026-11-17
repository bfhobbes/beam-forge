import type { Shortcut } from "./shortcut.js";

/**
 * Default bindings by action id, close to Figma and common laser-software conventions. An
 * action picks its binding up here when it registers, so features that land
 * later (undo, the drawing tools) only need to use the right id.
 * Ids without a registered action do nothing.
 */
export const defaultKeymap: Readonly<Record<string, Shortcut>> = {
  "app.palette": "Mod+K",
  "app.shortcuts": "Mod+/",

  "tool.select": "V",
  "tool.rectangle": "R",
  "tool.ellipse": "E",
  "tool.line": "L",
  "tool.polygon": "P",
  "tool.text": "T",

  "edit.undo": "Mod+Z",
  "edit.redo": "Mod+Shift+Z",
  "edit.cut": "Mod+X",
  "edit.copy": "Mod+C",
  "edit.paste": "Mod+V",
  "edit.duplicate": "Mod+D",
  "edit.delete": "Delete",
  "edit.select-all": "Mod+A",
  "edit.deselect": "Escape",
  "edit.nudge-left": "ArrowLeft",
  "edit.nudge-right": "ArrowRight",
  "edit.nudge-up": "ArrowUp",
  "edit.nudge-down": "ArrowDown",
  "edit.nudge-left-far": "Shift+ArrowLeft",
  "edit.nudge-right-far": "Shift+ArrowRight",
  "edit.nudge-up-far": "Shift+ArrowUp",
  "edit.nudge-down-far": "Shift+ArrowDown",
  "edit.group": "Mod+G",
  "edit.ungroup": "Mod+Shift+G",
  "edit.paste-in-place": "Mod+Shift+V",

  "arrange.bring-forward": "Mod+]",
  "arrange.send-backward": "Mod+[",
  "arrange.mirror-horizontal": "Shift+H",
  "arrange.mirror-vertical": "Shift+V",

  "file.new": "Mod+N",
  "file.open": "Mod+O",
  "file.save": "Mod+S",
  "file.save-as": "Mod+Shift+S",
  "file.import": "Mod+I",

  "view.zoom-in": "Mod+=",
  "view.zoom-out": "Mod+-",
  "view.zoom-fit": "Shift+1",
  "view.zoom-selection": "Shift+2",
};
