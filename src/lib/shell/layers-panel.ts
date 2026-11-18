import {
  addLayer,
  assignLayer,
  batch,
  DocumentError,
  editLayer,
  type Command,
} from "#lib/document/commands.js";
import type { DocumentEditor } from "#lib/document/editor.svelte.js";
import {
  layerForSlot,
  layerWithColor,
  LAYER_PALETTE,
  type LayerChanges,
} from "#lib/document/layers.js";
import type { CutLayer, DesignDocument } from "#lib/document/model.js";

export interface SwatchClick {
  /** The edit to make, or null when the layer already exists and nothing is selected. */
  command: Command | null;
  /** The layer the swatch stands for, to show in the panel. */
  layerId: string;
}

/**
 * What clicking palette colour `slot` does: with objects
 * selected it puts them on that colour's layer, adding the layer if the
 * document lacks it; with nothing selected it only adds the layer.
 */
export function clickSwatch(
  doc: DesignDocument,
  selected: readonly string[],
  slot: number,
): SwatchClick {
  const existing = layerWithColor(doc, LAYER_PALETTE[slot]);
  const layer = existing ?? layerForSlot(doc, slot);
  const steps: Command[] = [];
  if (!existing) steps.push(addLayer(layer));
  if (selected.length > 0) steps.push(assignLayer(doc, selected, layer.id));
  return {
    command: steps.length === 0 ? null : steps.length === 1 ? steps[0] : batch(steps),
    layerId: layer.id,
  };
}

/**
 * Applies a settings edit from a panel field. Repeated edits of one field
 * of one layer coalesce into a single undo step, like other inspector
 * edits. Returns why the edit was refused, or null if it was made.
 */
export function editLayerSetting(
  editor: DocumentEditor,
  layerId: string,
  changes: LayerChanges,
  coalesce = true,
): string | null {
  const field = Object.keys(changes).join(",");
  try {
    const done = editor.execute(
      editLayer(layerId, changes),
      coalesce ? { coalesce: `layer:${layerId}:${field}` } : {},
    );
    return done ? null : "Editing is paused while an update installs";
  } catch (error) {
    if (error instanceof DocumentError) return error.message;
    throw error;
  }
}

/**
 * The warning shown under a layer that cuts at constant power (`M3`), or
 * null for dynamic power (`M4`). If the link drops, GRBL stops wherever its
 * buffer runs out and, in `M3`, keeps the laser on there.
 */
export function constantPowerWarning(layer: Pick<CutLayer, "constantPower">): string | null {
  return layer.constantPower
    ? "If the USB connection drops during this layer, the laser stays on at this power until " +
        "the machine is reset or switched off. Use dynamic power unless you need full power at " +
        "corners."
    : null;
}
