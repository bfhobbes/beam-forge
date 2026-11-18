/**
 * Material presets on layers: what applying one changes, the settings a
 * layer saves as one, and whether a layer has been changed since. The library
 * itself is stored in Rust (`bf_core::presets`), per machine profile.
 */
import type { MaterialPreset } from "#lib/bindings/presets/MaterialPreset.js";
import type { Operation } from "#lib/bindings/presets/Operation.js";
import type { PresetSettings } from "#lib/bindings/presets/PresetSettings.js";
import type { LayerChanges } from "#lib/document/layers.js";
import type { CutLayer, LayerMode } from "#lib/document/model.js";

export type { MaterialPreset } from "#lib/bindings/presets/MaterialPreset.js";
export type { Operation } from "#lib/bindings/presets/Operation.js";
export type { PresetSettings } from "#lib/bindings/presets/PresetSettings.js";

export const OPERATION_LABELS: Record<Operation, string> = {
  cut: "Cut",
  score: "Score",
  engraveFill: "Engrave",
};

/** Cut and score follow outlines; engrave-fill scans across shapes. */
export function modeFor(operation: Operation): LayerMode {
  return operation === "engraveFill" ? "fill" : "line";
}

/** The operations a layer in `mode` can be saved as. */
export function operationsFor(mode: LayerMode): Operation[] {
  return mode === "fill" ? ["engraveFill"] : ["cut", "score"];
}

/** The operation "Save layer as preset" suggests: the layer's preset's, if it fits. */
export function defaultOperation(layer: CutLayer): Operation {
  const fits = operationsFor(layer.mode);
  const current = layer.preset?.settings.operation;
  return current && fits.includes(current) ? current : fits[0];
}

/** Millimetres without trailing zeros: `3`, `1.5`. */
function mm(value: number): string {
  return String(Number(value.toFixed(2)));
}

/** "3 mm Plywood · Cut", or "Basswood · Engrave" without a thickness. */
export function presetName(preset: Pick<MaterialPreset, "material" | "thickness" | "settings">) {
  const thickness = preset.thickness === null ? "" : `${mm(preset.thickness)} mm `;
  return `${thickness}${preset.material} · ${OPERATION_LABELS[preset.settings.operation]}`;
}

/** "300 mm/min · 100% · ×2 · air". */
export function settingsSummary(settings: PresetSettings): string {
  const parts = [`${settings.speed} mm/min`, `${settings.maxPower}%`];
  if (settings.passes > 1) parts.push(`×${settings.passes}`);
  if (settings.lineInterval !== undefined) parts.push(`${settings.lineInterval} mm`);
  if (settings.airAssist) parts.push("air");
  return parts.join(" · ");
}

/** The layer edit that applies `preset` and records where the settings came from. */
export function applyPreset(preset: MaterialPreset): LayerChanges {
  const { settings } = preset;
  return {
    mode: modeFor(settings.operation),
    speed: settings.speed,
    maxPower: settings.maxPower,
    minPower: settings.minPower,
    passes: settings.passes,
    airAssist: settings.airAssist,
    ...(settings.lineInterval === undefined ? {} : { lineInterval: settings.lineInterval }),
    preset: { id: preset.id, name: presetName(preset), settings },
  };
}

/** A layer's settings as a preset for `operation`. */
export function layerSettings(layer: CutLayer, operation: Operation): PresetSettings {
  return {
    operation,
    speed: layer.speed,
    maxPower: layer.maxPower,
    minPower: layer.minPower,
    passes: layer.passes,
    airAssist: layer.airAssist,
    ...(operation === "engraveFill" ? { lineInterval: layer.lineInterval } : {}),
  };
}

/** Whether `layer`'s settings differ from the preset it was given. */
export function presetModified(layer: CutLayer): boolean {
  const applied = layer.preset?.settings;
  if (!applied) return false;
  return (
    layer.mode !== modeFor(applied.operation) ||
    layer.speed !== applied.speed ||
    layer.maxPower !== applied.maxPower ||
    layer.minPower !== applied.minPower ||
    layer.passes !== applied.passes ||
    layer.airAssist !== applied.airAssist ||
    (applied.lineInterval !== undefined && layer.lineInterval !== applied.lineInterval)
  );
}

/** Presets whose name or notes contain every word of `query`, in library order. */
export function searchPresets(presets: readonly MaterialPreset[], query: string): MaterialPreset[] {
  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  if (words.length === 0) return [...presets];
  return presets.filter((preset) => {
    const text = `${presetName(preset)} ${preset.notes}`.toLowerCase();
    return words.every((word) => text.includes(word));
  });
}
